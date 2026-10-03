//! Per-dev-machine Redis Streams outbox verbs (D-14, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/services/machine_outbox.py:111-393`
//! (the Redis operations; key builders, `_VALID_TYPES` /
//! `_OFFLINE_REJECT`, `_serialize` and `_decode_read_result` live in
//! `pidash_types::runner_sessions`, the session-row SQL in
//! [`super::models`]). The machine twin of [`super::outbox`] with a
//! smaller valid-type set (`welcome` / `ping` / `create_runner` /
//! `config_push`), no consumer-claim/reap/trim verbs (they do not
//! exist on the machine side and are not invented here), plus
//! command-result tracking.
//!
//! # Calling conventions
//!
//! Every verb takes `client: Option<&redis::Client>` first. `None`
//! mirrors `redis_instance()` returning `None` (missing `REDIS_URL`)
//! and yields the same default Python returns without touching the
//! network (`None`, `0`, `[]`, `false`). Callers build the client
//! from settings (the `auth_session::magic` /
//! `api::runner_runs::LivePorts` precedent — `db::redis::RedisHandle`
//! exposes no stream primitives and foundation crates are read-only)
//! and pass `None` when `REDIS_URL` is unset or unparsable.
//!
//! Tunables Python reads off Django settings come in as
//! `&RunnerSettings` (populated from the same env names with the
//! same defaults): the offline `maxlen` / TTL for
//! [`enqueue_for_machine`] and 2x `access_token_ttl_secs` for
//! [`mark_pel_drained`]. The command-result TTL (900s) is a module
//! constant on both sides
//! (`keys::machine::COMMAND_RESULT_TTL_SECS`).
//!
//! IDs are `&str` (mirroring Python's `UUID | str` unions), except
//! [`active_session_id_for_machine`] and [`enqueue_for_machine`],
//! which bind a typed [`uuid::Uuid`] to the session lookup.
//!
//! All commands are raw `redis::cmd` spells (the `streams`
//! `AsyncCommands` helpers are feature-gated off): the argv order
//! matches redis-py 5.0.4 byte for byte (`XADD … MAXLEN ~ N * …`,
//! `XREADGROUP … COUNT n BLOCK ms STREAMS …`, `SET … EX n`),
//! pinned by the scripted-server tests below.
//!
//! # Failure policy
//!
//! Each verb mirrors its Python `try`/`except` exactly: transport
//! failures swallowed in Python (with `logger.exception`) return the
//! same value here (logged with `tracing::warn!`); failures that
//! propagate in Python surface as [`MachineOutboxError`]:
//!
//! | verb | `None` client | transport error | reply-shape error |
//! | ---- | ------------- | --------------- | ----------------- |
//! | `ensure_stream_group` | `Ok(())` | BUSYGROUP → `Ok(())`, else `Err` | — |
//! | `active_session_id_for_machine` | — (SQL) | `Err(Db)` | — |
//! | `enqueue_for_machine` | warn + `Ok(None)` | `Err` | — |
//! | `drain_offline_into_live` | `Ok(0)` | `Err` | `Err(InvalidUtf8)` |
//! | `read_for_session` | `Ok(vec![])` | ensure → `Err`; `XREADGROUP` → log + `Ok(vec![])` | `Err` (decode is outside the `try`) |
//! | `ack_for_session` | `Ok(0)` | `Err` | — |
//! | `mark_pel_drained` / `clear_session_marker` | `Ok(())` | `Err` | — |
//! | `is_pel_drained` | `Ok(false)` | `Err` | — |
//! | `set_command_result` | warn + `Ok(())` | `Err` | — |
//! | `get_command_result` | `Ok(None)` | `Err` | corrupt → `Ok(None)` |
//! | `publish_session_eviction` | `Ok(())` | `Err` | — |
//! | `delete_machine_stream` | `Ok(())` | `Err` | — |
//!
//! # Collapsed twins
//!
//! `_ensure_group` / `_aensure_group` collapse into the single async
//! [`ensure_stream_group`], and `read_for_session` /
//! `aread_for_session` into the single async [`read_for_session`]:
//! Rust has no sync Redis client here, and each twin pair issues the
//! identical command.
//!
//! # Command results
//!
//! `set_command_result` stores `json.dumps(payload, default=str)`
//! with `EX 900`. The rendering is a private `dumps_*` port below:
//! the shared spaced-ASCII renderer (`render_spaced_ascii`) is
//! `pub(crate)` inside `pidash-types`, and this issue allows no
//! edits outside this file plus wiring, so the dozen lines it needs
//! (default separators, `ensure_ascii` quoting, CPython float
//! spelling) are spelled out here instead of shared. `default=str`
//! has no Rust-side work to do: every value reaching it is already a
//! `serde_json::Value`, and every Python caller `str()`-ifies its
//! UUIDs and datetimes before building the dict.
//!
//! `get_command_result` returns whatever `json.loads` parses —
//! `Option<Value>`, since Python's `Optional[Dict]` annotation does
//! not stop a non-object literal passing through. Missing keys,
//! empty values, undecodable bytes (`UnicodeDecodeError` is a
//! `ValueError`) and corrupt payloads all read as `None`. One known
//! edge: bare `NaN`/`Infinity` literals (storable only from Python,
//! via non-finite floats — `serde_json::Number` cannot hold them)
//! read as corrupt (`None`) here where Python re-parses them.
//!
//! # Ported bugs
//!
//! None on the machine side: TRACE.md's bug list (async-Redis flake,
//! unhandled drain-dispatch `RunnerOfflineError`, the `XAUTOCLAIM`
//! JUSTID spin, the reap pending-count sum) touches only the runner
//! outbox and the poll views. The machine open asymmetries the
//! models layer observed (no `_bound_txn_waits`, the double
//! `timezone.now()`) are handler-owned, not this layer's to fix.
//!
//! Fixture replayed by the tests below:
//! `rust-api/fixtures/runner_sessions/fx-rses-05-outbox-ops.json`
//! (FX-RSES-05, machine + `command_result` sections; runner sections
//! are PIDASHCONV-550's).

use std::fmt::Write as _;

use redis::aio::MultiplexedConnection;
use serde_json::{Map, Value};
use thiserror::Error as ThisError;

use pidash_types::runner_sessions::keys::machine as keys;
use pidash_types::runner_sessions::{
    decode_read_result, eviction_body, is_offline_reject_machine_type, is_valid_machine_type,
    serialize, DecodeError, DecodedMessage, StreamEntry, StreamRead,
};

use super::models::machine_session;
use crate::config::RunnerSettings;

/// Failures that propagate out of the machine-outbox verbs — exactly
/// the failures Python lets raise (see the module failure-policy
/// table). Everything Python swallows with `logger.exception` (or
/// with `get_command_result`'s `except (TypeError, ValueError)`)
/// stays a plain return value, never one of these.
#[derive(Debug, ThisError)]
pub enum MachineOutboxError {
    /// `ValueError: unknown machine message type …` from
    /// [`enqueue_for_machine`] (`machine_outbox.py:172`).
    #[error("unknown machine message type '{0}'")]
    UnknownMessageType(String),

    /// [`enqueue_for_machine`] for an offline machine with a
    /// non-queueable type (`machine_outbox.py:186`). The message
    /// matches `MachineOfflineError` verbatim.
    #[error("dev machine {dev_machine_id} is offline; type '{message_type}' cannot queue")]
    MachineOffline {
        dev_machine_id: String,
        message_type: String,
    },

    /// A Redis transport failure Python lets propagate (every verb
    /// without a `try` around the command, plus non-`BUSYGROUP`
    /// group creation).
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    /// A session-lookup SQL failure (`machine_outbox.py:148-155` has
    /// no `try`).
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    /// `read_for_session` payload decode failure: invalid UTF-8 or a
    /// non-object payload with missing `mid`/`type`
    /// (`_decode_read_result`'s `.decode()` calls sit outside its
    /// `try`, `outbox.py:164-189`, reused by the machine outbox per
    /// `machine_outbox.py:23-25,37-40`).
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),

    /// Non-UTF-8 bytes where Python's `.decode()` would raise: drain
    /// field bytes (`machine_outbox.py:213-218`).
    #[error("invalid utf-8: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// A reply shape a real server never sends, where Python would
    /// raise `TypeError`/`ValueError`/`AttributeError` out of the
    /// parsing code (non-array `XREADGROUP` replies, unpack
    /// failures).
    #[error("unexpected redis reply ({0})")]
    UnexpectedReply(&'static str),
}

/// Bulk bytes of a reply value (bulk or simple strings — servers
/// send bulk for every value parsed here).
fn as_bytes(value: &redis::Value) -> Option<&[u8]> {
    match value {
        redis::Value::BulkString(bytes) => Some(bytes),
        redis::Value::SimpleString(text) => Some(text.as_bytes()),
        _ => None,
    }
}

/// Array elements of a reply value.
fn as_array(value: &redis::Value) -> Option<&[redis::Value]> {
    match value {
        redis::Value::Array(items) => Some(items),
        _ => None,
    }
}

/// Open a multiplexed connection for one verb call (Python issues a
/// verb's commands over its single client).
async fn connect(client: &redis::Client) -> Result<MultiplexedConnection, redis::RedisError> {
    client.get_multiplexed_async_connection().await
}

/// Create the persistent stream + consumer group if missing
/// (`machine_outbox.py:111-130`). `BUSYGROUP` means the group
/// exists. The sync/async twins collapse: one async body serves all
/// verbs.
async fn ensure_group(
    connection: &mut MultiplexedConnection,
    dev_machine_id: &str,
) -> Result<(), redis::RedisError> {
    let mut create = redis::cmd("XGROUP");
    create
        .arg("CREATE")
        .arg(keys::stream_key(dev_machine_id))
        .arg(keys::group_name(dev_machine_id))
        .arg("$")
        .arg("MKSTREAM");
    let created: Result<(), redis::RedisError> = create.query_async(connection).await;
    match created {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("BUSYGROUP") => Ok(()),
        Err(error) => Err(error),
    }
}

/// `json.dumps(payload, default=str)` (`machine_outbox.py:348-352`):
/// default separators (`', '`, `': '`), `ensure_ascii` quoting, the
/// payload's key order. See the module docs for why this renderer is
/// private to this file.
fn dumps_payload(payload: &Map<String, Value>) -> String {
    let mut out = String::new();
    dumps_map(payload, &mut out);
    out
}

fn dumps_map(map: &Map<String, Value>, out: &mut String) {
    out.push('{');
    for (i, (key, value)) in map.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        dumps_string(key, out);
        out.push_str(": ");
        dumps_value(value, out);
    }
    out.push('}');
}

fn dumps_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&dumps_number(n)),
        Value::String(s) => dumps_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => dumps_map(map, out),
    }
}

fn dumps_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    match n.as_f64() {
        Some(f) => dumps_float(f),
        // Unreachable without `arbitrary_precision`: every Number is
        // i64, u64 or f64.
        None => n.to_string(),
    }
}

/// CPython `repr()` spelling for a finite float, with `json`-mode
/// (`NaN`/`Infinity`) non-finite words (`json.dumps` defaults to
/// `allow_nan=True`).
///
/// Shortest digits come from Ryū (via `serde_json::Number`, already
/// in the dep tree): the shortest spelling that round-trips, ties
/// broken toward the true value — exactly `repr`'s rule. Rust's own
/// `{:e}` is shortest too but breaks ties differently (it renders
/// `153838026194641.13` where `repr` gives `...412`), so it cannot
/// be the digit source. Only the fixed/exponent choice (`-4 <= e10
/// <= 15` is fixed) and the exponent shape (`e±XX`, two digits
/// minimum) are applied here, so no re-rounding occurs.
fn dumps_float(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        if f.is_sign_negative() {
            return "-Infinity".to_string();
        }
        return "Infinity".to_string();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let ryu = serde_json::Number::from_f64(f)
        .expect("finite float")
        .to_string();
    let neg = f.is_sign_negative();
    let mant = ryu.trim_start_matches('-');
    let (mant, exp10) = match mant.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().expect("ryu exponent")),
        None => (mant, 0),
    };
    let frac_len = mant.split_once('.').map_or(0, |(_, fr)| fr.len() as i32);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_start_matches('0').to_string();
    // `f` is finite and nonzero, so a nonzero digit always survives.
    debug_assert!(!digits.is_empty());
    let exp = exp10 - frac_len + digits.len() as i32 - 1;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if (-4..=15).contains(&exp) {
        // Fixed notation; the point sits after `exp + 1` digits.
        let point = exp + 1;
        if point <= 0 {
            out.push_str("0.");
            for _ in point..0 {
                out.push('0');
            }
            out.push_str(&digits);
        } else if point as usize >= digits.len() {
            out.push_str(&digits);
            for _ in digits.len()..point as usize {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            let point = point as usize;
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{exp:+03}");
    }
    out
}

/// CPython `json.dumps` string quoting with `ensure_ascii=True`:
/// short escapes, `\u00xx` for other C0 controls and DEL, `\uXXXX`
/// for everything non-ASCII (astral chars as surrogate pairs), all
/// lowercase hex.
fn dumps_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if (c as u32) >= 0x80 => {
                let n = c as u32;
                if n > 0xffff {
                    let n = n - 0x10000;
                    let _ = write!(
                        out,
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (n >> 10),
                        0xdc00 + (n & 0x3ff)
                    );
                } else {
                    let _ = write!(out, "\\u{n:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Idempotent `XGROUP CREATE … MKSTREAM` for the machine stream
/// (`machine_outbox.py:133-138`).
pub async fn ensure_stream_group(
    client: Option<&redis::Client>,
    dev_machine_id: &str,
) -> Result<(), MachineOutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    ensure_group(&mut connection, dev_machine_id).await?;
    Ok(())
}

/// The active session id for a dev machine, or `None`
/// (`machine_outbox.py:144-155`). Python returns `str(sid)`; the
/// typed [`uuid::Uuid`] renders identically and every caller only
/// tests presence.
pub async fn active_session_id_for_machine<'e, E>(
    ex: E,
    dev_machine_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, MachineOutboxError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id: Option<uuid::Uuid> = sqlx::query_scalar(machine_session::ACTIVE_ID_SQL)
        .bind(dev_machine_id)
        .fetch_optional(ex)
        .await?;
    Ok(id)
}

/// Enqueue a machine-scoped control message
/// (`machine_outbox.py:161-193`).
///
/// Returns the live-stream id when a session is active, `None` when
/// buffered offline or when `client` is `None` (warn-logged, before
/// the session lookup — no SQL runs then). Raises
/// [`MachineOutboxError::UnknownMessageType`] for types outside
/// `_VALID_TYPES` (checked first, even with no client) and
/// [`MachineOutboxError::MachineOffline`] for offline-reject types
/// with no active session.
pub async fn enqueue_for_machine<'e, E>(
    client: Option<&redis::Client>,
    ex: E,
    runner: &RunnerSettings,
    dev_machine_id: uuid::Uuid,
    message: &Map<String, Value>,
) -> Result<Option<String>, MachineOutboxError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    // `message.get("mid") or uuid4()`: the fresh id is minted up
    // front and used exactly when the mid is missing or falsy
    // (mint-and-discard is unobservable).
    let fresh_mid = uuid::Uuid::new_v4().to_string();
    let fields = serialize(message, &fresh_mid);
    // The body's `type` is the message's `type` (`mid` insertion
    // cannot touch it), so the serialized type doubles as the
    // validated one — one spelling, no drift.
    if !is_valid_machine_type(&fields.msg_type) {
        return Err(MachineOutboxError::UnknownMessageType(fields.msg_type));
    }
    let Some(client) = client else {
        tracing::warn!(
            dev_machine_id = %dev_machine_id,
            "redis unavailable; cannot enqueue for dev machine"
        );
        return Ok(None);
    };
    let mid = dev_machine_id.to_string();

    if active_session_id_for_machine(ex, dev_machine_id)
        .await?
        .is_some()
    {
        let mut connection = connect(client).await?;
        ensure_group(&mut connection, &mid).await?;
        let mut xadd = redis::cmd("XADD");
        xadd.arg(keys::stream_key(&mid))
            .arg("*")
            .arg("mid")
            .arg(&fields.mid)
            .arg("type")
            .arg(&fields.msg_type)
            .arg("payload")
            .arg(&fields.payload);
        let stream_id: String = xadd.query_async(&mut connection).await?;
        return Ok(Some(stream_id));
    }

    if is_offline_reject_machine_type(&fields.msg_type) {
        return Err(MachineOutboxError::MachineOffline {
            dev_machine_id: mid,
            message_type: fields.msg_type,
        });
    }

    let mut connection = connect(client).await?;
    let key = keys::offline_stream_key(&mid);
    let mut xadd = redis::cmd("XADD");
    xadd.arg(&key)
        .arg("MAXLEN")
        .arg("~")
        .arg(runner.offline_stream_maxlen)
        .arg("*")
        .arg("mid")
        .arg(&fields.mid)
        .arg("type")
        .arg(&fields.msg_type)
        .arg("payload")
        .arg(&fields.payload);
    let _: () = xadd.query_async(&mut connection).await?;
    let mut expire = redis::cmd("EXPIRE");
    expire.arg(&key).arg(runner.offline_stream_ttl_secs);
    let _: () = expire.query_async(&mut connection).await?;
    Ok(None)
}

/// Move every entry from the offline buffer into the live stream
/// (`machine_outbox.py:196-222`). Returns the number of entries
/// moved. Field bytes round-trip through UTF-8 exactly like
/// Python's decode-then-`XADD` (non-UTF-8 raises there, `Err`
/// here).
pub async fn drain_offline_into_live(
    client: Option<&redis::Client>,
    dev_machine_id: &str,
) -> Result<usize, MachineOutboxError> {
    let Some(client) = client else {
        return Ok(0);
    };
    let mut connection = connect(client).await?;
    let okey = keys::offline_stream_key(dev_machine_id);
    let mut xrange = redis::cmd("XRANGE");
    xrange.arg(&okey).arg("-").arg("+");
    let reply: redis::Value = xrange.query_async(&mut connection).await?;
    let entries = match &reply {
        redis::Value::Nil => &[][..],
        redis::Value::Array(items) => &items[..],
        _ => {
            return Err(MachineOutboxError::UnexpectedReply(
                "XRANGE reply not an array",
            ));
        }
    };
    if entries.is_empty() {
        return Ok(0);
    }
    ensure_group(&mut connection, dev_machine_id).await?;
    let sk = keys::stream_key(dev_machine_id);
    let mut moved = 0usize;
    for entry in entries {
        // `for _, fields in entries`: a non-pair entry fails to
        // unpack in Python, `Err` here.
        let pair = match as_array(entry) {
            Some(pair) if pair.len() == 2 => pair,
            _ => {
                return Err(MachineOutboxError::UnexpectedReply(
                    "XRANGE entry not an id/fields pair",
                ));
            }
        };
        let flat = match as_array(&pair[1]) {
            Some(flat) => flat,
            None => {
                return Err(MachineOutboxError::UnexpectedReply(
                    "XRANGE fields not an array",
                ));
            }
        };
        let mut xadd = redis::cmd("XADD");
        xadd.arg(&sk).arg("*");
        for pair in flat.chunks(2) {
            if pair.len() < 2 {
                // `pairs_to_dict` drops a trailing odd element.
                break;
            }
            let key = match as_bytes(&pair[0]) {
                Some(key) => std::str::from_utf8(key)?,
                None => {
                    return Err(MachineOutboxError::UnexpectedReply(
                        "XRANGE field name not bytes",
                    ));
                }
            };
            let value = match as_bytes(&pair[1]) {
                Some(value) => std::str::from_utf8(value)?,
                None => {
                    return Err(MachineOutboxError::UnexpectedReply(
                        "XRANGE field value not bytes",
                    ));
                }
            };
            xadd.arg(key).arg(value);
        }
        let _: () = xadd.query_async(&mut connection).await?;
        moved += 1;
    }
    let mut delete = redis::cmd("DEL");
    delete.arg(&okey);
    let _: () = delete.query_async(&mut connection).await?;
    Ok(moved)
}

/// Set the per-session PEL-drained marker with a TTL of
/// 2x `access_token_ttl_secs` (`machine_outbox.py:225-230`).
pub async fn mark_pel_drained(
    client: Option<&redis::Client>,
    runner: &RunnerSettings,
    session_id: &str,
) -> Result<(), MachineOutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut set = redis::cmd("SET");
    set.arg(keys::session_pel_drained_key(session_id))
        .arg("1")
        .arg("EX")
        .arg(runner.access_token_ttl_secs * 2);
    let _: () = set.query_async(&mut connection).await?;
    Ok(())
}

/// Whether the per-session PEL-drained marker exists
/// (`machine_outbox.py:233-237`).
pub async fn is_pel_drained(
    client: Option<&redis::Client>,
    session_id: &str,
) -> Result<bool, MachineOutboxError> {
    let Some(client) = client else {
        return Ok(false);
    };
    let mut connection = connect(client).await?;
    let mut exists = redis::cmd("EXISTS");
    exists.arg(keys::session_pel_drained_key(session_id));
    let found: i64 = exists.query_async(&mut connection).await?;
    Ok(found != 0)
}

/// Delete the per-session PEL-drained marker
/// (`machine_outbox.py:240-244`).
pub async fn clear_session_marker(
    client: Option<&redis::Client>,
    session_id: &str,
) -> Result<(), MachineOutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut delete = redis::cmd("DEL");
    delete.arg(keys::session_pel_drained_key(session_id));
    let _: () = delete.query_async(&mut connection).await?;
    Ok(())
}

/// Convert an `XREADGROUP` reply into the pre-decode sections
/// (`outbox.py:164-189` input shape, shared by the machine outbox).
/// `nil` (block timeout, no entries) reads as absent, like Python's
/// `if not result`.
fn parse_read_reply(reply: &redis::Value) -> Result<Option<Vec<StreamRead>>, MachineOutboxError> {
    match reply {
        redis::Value::Nil => Ok(None),
        redis::Value::Array(streams) => {
            let mut out = Vec::with_capacity(streams.len());
            for stream in streams {
                // `for _, entries in result`.
                let section = match as_array(stream) {
                    Some(section) if section.len() == 2 => section,
                    _ => {
                        return Err(MachineOutboxError::UnexpectedReply(
                            "XREADGROUP stream section not a name/entries pair",
                        ));
                    }
                };
                let name = match as_bytes(&section[0]) {
                    Some(name) => name.to_vec(),
                    None => {
                        return Err(MachineOutboxError::UnexpectedReply(
                            "XREADGROUP stream name not bytes",
                        ));
                    }
                };
                let raw_entries = match as_array(&section[1]) {
                    Some(entries) => entries,
                    None => {
                        return Err(MachineOutboxError::UnexpectedReply(
                            "XREADGROUP entries not an array",
                        ));
                    }
                };
                let mut entries = Vec::with_capacity(raw_entries.len());
                for raw in raw_entries {
                    // `for stream_id, fields in entries`.
                    let pair = match as_array(raw) {
                        Some(pair) if pair.len() == 2 => pair,
                        _ => {
                            return Err(MachineOutboxError::UnexpectedReply(
                                "XREADGROUP entry not an id/fields pair",
                            ));
                        }
                    };
                    let id = match as_bytes(&pair[0]) {
                        Some(id) => id.to_vec(),
                        None => {
                            return Err(MachineOutboxError::UnexpectedReply(
                                "XREADGROUP entry id not bytes",
                            ));
                        }
                    };
                    let flat = match as_array(&pair[1]) {
                        Some(flat) => flat,
                        None => {
                            return Err(MachineOutboxError::UnexpectedReply(
                                "XREADGROUP fields not an array",
                            ));
                        }
                    };
                    let mut fields = Vec::with_capacity(flat.len() / 2);
                    for pair in flat.chunks(2) {
                        if pair.len() < 2 {
                            break;
                        }
                        let key = match as_bytes(&pair[0]) {
                            Some(key) => key.to_vec(),
                            None => {
                                return Err(MachineOutboxError::UnexpectedReply(
                                    "XREADGROUP field name not bytes",
                                ));
                            }
                        };
                        let value = match as_bytes(&pair[1]) {
                            Some(value) => value.to_vec(),
                            None => {
                                return Err(MachineOutboxError::UnexpectedReply(
                                    "XREADGROUP field value not bytes",
                                ));
                            }
                        };
                        fields.push((key, value));
                    }
                    entries.push(StreamEntry { id, fields });
                }
                out.push(StreamRead { name, entries });
            }
            Ok(Some(out))
        }
        _ => Err(MachineOutboxError::UnexpectedReply(
            "XREADGROUP reply neither nil nor array",
        )),
    }
}

/// One `XREADGROUP` against the machine stream
/// (`machine_outbox.py:247-308`, sync and async twins collapsed).
/// `use_zero` reads `0` (the consumer's PEL replay), otherwise `>`
/// (new entries). Transport failures log and return no entries;
/// decode failures propagate.
pub async fn read_for_session(
    client: Option<&redis::Client>,
    dev_machine_id: &str,
    session_id: &str,
    block_ms: i64,
    count: i64,
    use_zero: bool,
) -> Result<Vec<DecodedMessage>, MachineOutboxError> {
    let Some(client) = client else {
        return Ok(Vec::new());
    };
    let mut connection = connect(client).await?;
    ensure_group(&mut connection, dev_machine_id).await?;
    let last_id = if use_zero { "0" } else { ">" };
    let mut xreadgroup = redis::cmd("XREADGROUP");
    xreadgroup
        .arg("GROUP")
        .arg(keys::group_name(dev_machine_id))
        .arg(keys::consumer_name(session_id))
        .arg("COUNT")
        .arg(count)
        .arg("BLOCK")
        .arg(block_ms)
        .arg("STREAMS")
        .arg(keys::stream_key(dev_machine_id))
        .arg(last_id);
    let reply: redis::Value = match xreadgroup.query_async(&mut connection).await {
        Ok(reply) => reply,
        Err(error) => {
            tracing::warn!(%error, dev_machine_id, "xreadgroup failed for dev machine");
            return Ok(Vec::new());
        }
    };
    match parse_read_reply(&reply)? {
        Some(streams) => Ok(decode_read_result(Some(&streams))?),
        None => Ok(Vec::new()),
    }
}

/// Multi-id `XACK`; returns the count of removed PEL entries
/// (`machine_outbox.py:311-323`). Empty (or all-falsy) ids return
/// `0` with no Redis call.
pub async fn ack_for_session(
    client: Option<&redis::Client>,
    dev_machine_id: &str,
    stream_ids: &[String],
) -> Result<usize, MachineOutboxError> {
    let ids: Vec<&str> = stream_ids
        .iter()
        .map(String::as_str)
        .filter(|id| !id.is_empty())
        .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let Some(client) = client else {
        return Ok(0);
    };
    let mut connection = connect(client).await?;
    let mut xack = redis::cmd("XACK");
    xack.arg(keys::stream_key(dev_machine_id))
        .arg(keys::group_name(dev_machine_id));
    for id in &ids {
        xack.arg(id);
    }
    let acked: i64 = xack.query_async(&mut connection).await?;
    Ok(usize::try_from(acked).unwrap_or(0))
}

/// Record a machine-command result (`machine_outbox.py:343-352`):
/// `json.dumps(payload, default=str)` under
/// `machine_cmd_result:<request_id>` with a 900s TTL. Consumed by
/// D-13's `machine_commands` result endpoint (PIDASHCONV-593).
pub async fn set_command_result(
    client: Option<&redis::Client>,
    request_id: &str,
    payload: &Map<String, Value>,
) -> Result<(), MachineOutboxError> {
    let Some(client) = client else {
        tracing::warn!(
            request_id,
            "redis unavailable; cannot record command result"
        );
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut set = redis::cmd("SET");
    set.arg(keys::command_result_key(request_id))
        .arg(dumps_payload(payload))
        .arg("EX")
        .arg(keys::COMMAND_RESULT_TTL_SECS);
    let _: () = set.query_async(&mut connection).await?;
    Ok(())
}

/// Read a machine-command result (`machine_outbox.py:355-367`):
/// whatever `json.loads` parses, `None` for missing keys, empty
/// values and corrupt (or undecodable) payloads.
pub async fn get_command_result(
    client: Option<&redis::Client>,
    request_id: &str,
) -> Result<Option<Value>, MachineOutboxError> {
    let Some(client) = client else {
        return Ok(None);
    };
    let mut connection = connect(client).await?;
    let mut get = redis::cmd("GET");
    get.arg(keys::command_result_key(request_id));
    let reply: redis::Value = get.query_async(&mut connection).await?;
    // `if not raw: return None` — missing keys (nil) read as
    // unknown, like an expired key.
    let raw = match &reply {
        redis::Value::Nil => return Ok(None),
        redis::Value::BulkString(raw) => raw.as_slice(),
        redis::Value::SimpleString(text) => text.as_bytes(),
        // GET never yields these from a real server; Python's
        // `json.loads` would raise TypeError on them, which the
        // `except (TypeError, ValueError)` swallows to None.
        _ => return Ok(None),
    };
    if raw.is_empty() {
        return Ok(None);
    }
    // Non-UTF-8 bytes raise UnicodeDecodeError in Python — a
    // ValueError subclass, so the corrupt read is None, never Err.
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(_) => return Ok(None),
    };
    Ok(serde_json::from_str(text).ok())
}

/// Publish a session-eviction notice
/// (`machine_outbox.py:373-385`). The subscriber count is ignored,
/// like Python's bare `publish`.
pub async fn publish_session_eviction(
    client: Option<&redis::Client>,
    dev_machine_id: &str,
    old_session_id: Option<&str>,
    new_session_id: &str,
) -> Result<(), MachineOutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut publish = redis::cmd("PUBLISH");
    publish
        .arg(keys::session_eviction_channel(dev_machine_id))
        .arg(eviction_body(old_session_id, new_session_id));
    let _: () = publish.query_async(&mut connection).await?;
    Ok(())
}

/// Delete a machine's live and offline streams
/// (`machine_outbox.py:388-393`).
pub async fn delete_machine_stream(
    client: Option<&redis::Client>,
    dev_machine_id: &str,
) -> Result<(), MachineOutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut delete_live = redis::cmd("DEL");
    delete_live.arg(keys::stream_key(dev_machine_id));
    let _: () = delete_live.query_async(&mut connection).await?;
    let mut delete_offline = redis::cmd("DEL");
    delete_offline.arg(keys::offline_stream_key(dev_machine_id));
    let _: () = delete_offline.query_async(&mut connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    static FX05: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-05-outbox-ops.json");

    fn fixture() -> Value {
        serde_json::from_str(FX05).expect("FX-RSES-05 parses")
    }

    fn runner_settings() -> RunnerSettings {
        Settings::test_defaults().runner
    }

    fn message(msg_type: &str) -> Map<String, Value> {
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String(msg_type.to_string()));
        message.insert("mid".to_string(), Value::String("test-mid-1".to_string()));
        message
    }

    /// A pool that can never connect (TEST-NET port 1): any SQL
    /// attempt fails, so `Ok` results prove no SQL ran. Built inside
    /// a runtime because even lazy construction needs a Tokio
    /// context; never used for I/O afterwards.
    fn dead_pool() -> sqlx::PgPool {
        block_on(async {
            sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/machine_outbox_hermetic")
                .expect("lazy pool parses")
        })
    }

    fn fresh_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Drive a hermetic future to completion (all callers resolve
    /// without touching I/O, so a bare current-thread runtime
    /// suffices).
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    // -- hermetic: validation, messages, None-paths, parsers --------

    #[test]
    fn unknown_type_rejected_before_any_io() {
        // Validation precedes even the client check: `None` plus an
        // unknown type still raises, and the dead pool proves no SQL.
        let pool = dead_pool();
        let runner = runner_settings();
        let mid = uuid::Uuid::new_v4();
        let fx = fixture();
        let error = block_on(enqueue_for_machine(
            None,
            &pool,
            &runner,
            mid,
            &message("nope"),
        ))
        .expect_err("unknown type raises");
        let expected = fx["enqueue_unknown_type"]["machine"]
            .as_str()
            .expect("fixture vector");
        assert_eq!(format!("ValueError: {error}"), expected);

        // A missing type reads as `""` (`message.get("type") or ""`).
        let error = block_on(enqueue_for_machine(None, &pool, &runner, mid, &Map::new()))
            .expect_err("missing type raises");
        assert_eq!(
            format!("ValueError: {error}"),
            "ValueError: unknown machine message type ''"
        );
    }

    #[test]
    fn offline_error_message_matches_python() {
        let fx = fixture();
        let mid = "12aad42f-f469-4813-bfe2-8847b93b0500";
        for msg_type in ["create_runner", "config_push"] {
            let error = MachineOutboxError::MachineOffline {
                dev_machine_id: mid.to_string(),
                message_type: msg_type.to_string(),
            };
            let expected = fx["machine_enqueue_offline_reject"][msg_type]
                .as_str()
                .expect("fixture vector");
            assert_eq!(format!("MachineOfflineError: {error}"), expected);
        }
        assert!(fx["machine_enqueue_offline_reject"]["ping_ok"].is_null());
    }

    #[test]
    fn redis_none_returns_defaults_without_io() {
        let pool = dead_pool();
        let runner = runner_settings();
        let mid = uuid::Uuid::new_v4();
        let mid_str = mid.to_string();
        let sid = fresh_id();
        let fx = fixture();
        let none = &fx["redis_none"];

        let enqueue = block_on(enqueue_for_machine(
            None,
            &pool,
            &runner,
            mid,
            &message("ping"),
        ));
        assert_eq!(enqueue.expect("enqueue None"), None);
        assert!(none["machine_enqueue"].is_null());

        assert_eq!(
            block_on(drain_offline_into_live(None, &mid_str)).expect("drain"),
            0
        );
        assert_eq!(none["drain"], json!(0));
        block_on(mark_pel_drained(None, &runner, &sid)).expect("mark");
        assert!(
            !block_on(is_pel_drained(None, &sid)).expect("is drained"),
            "None reads as not drained"
        );
        assert_eq!(none["is_pel_drained"], json!(false));
        block_on(clear_session_marker(None, &sid)).expect("clear");
        assert!(
            block_on(read_for_session(None, &mid_str, &sid, 0, 100, false))
                .expect("read")
                .is_empty()
        );
        assert_eq!(none["read"], json!([]));
        assert_eq!(
            block_on(ack_for_session(None, &mid_str, &["1-0".to_string()])).expect("ack"),
            0
        );
        assert_eq!(none["ack"], json!(0));
        block_on(publish_session_eviction(None, &mid_str, None, &sid)).expect("publish");
        block_on(delete_machine_stream(None, &mid_str)).expect("delete");
        block_on(ensure_stream_group(None, &mid_str)).expect("ensure");
        assert!(none["ensure_group"].is_null());
        let mut payload = Map::new();
        payload.insert("status".to_string(), Value::String("ok".to_string()));
        block_on(set_command_result(None, "req-1", &payload)).expect("set result");
        assert_eq!(
            block_on(get_command_result(None, "req-1")).expect("get result"),
            None
        );
        assert!(none["get_command_result"].is_null());
        assert_eq!(none["note"], json!("void fns returned without raising"));
    }

    #[test]
    fn empty_ack_skips_redis_entirely() {
        // A dead-port client proves no connection is even attempted:
        // the falsy checks precede everything.
        let client = redis::Client::open("redis://127.0.0.1:1/").expect("dead-port client parses");
        let mid = fresh_id();
        assert_eq!(
            block_on(ack_for_session(Some(&client), &mid, &[])).expect("ack"),
            0
        );
        assert_eq!(
            block_on(ack_for_session(
                Some(&client),
                &mid,
                &["".to_string(), String::new()]
            ))
            .expect("ack falsy"),
            0
        );
    }

    #[test]
    fn read_reply_parse_vectors() {
        use redis::Value::{Array, BulkString, Int, Nil};
        let bytes = |s: &[u8]| BulkString(s.to_vec());

        assert_eq!(
            parse_read_reply(&Nil).expect("nil"),
            None,
            "nil reads as absent"
        );
        let streams = parse_read_reply(&Array(vec![]))
            .expect("empty")
            .expect("some");
        assert!(streams.is_empty());

        // One stream, one entry, fields round-trip raw (decoding is
        // the shapes crate's job).
        let reply = Array(vec![Array(vec![
            bytes(b"machine_stream:m"),
            Array(vec![Array(vec![
                bytes(b"9-0"),
                Array(vec![
                    bytes(b"mid"),
                    bytes(b"m1"),
                    bytes(b"type"),
                    bytes(b"ping"),
                    bytes(b"payload"),
                    bytes(b"{}"),
                    bytes(b"odd-trailing"),
                ]),
            ])]),
        ])]);
        let streams = parse_read_reply(&reply).expect("parses").expect("some");
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].entries.len(), 1);
        assert_eq!(streams[0].entries[0].id, b"9-0");
        assert_eq!(
            streams[0].entries[0].fields,
            vec![
                (b"mid".to_vec(), b"m1".to_vec()),
                (b"type".to_vec(), b"ping".to_vec()),
                (b"payload".to_vec(), b"{}".to_vec()),
            ],
            "trailing odd field dropped like pairs_to_dict"
        );

        // Malformed shapes raise in Python (`Err` here).
        assert!(parse_read_reply(&Int(1)).is_err());
        assert!(parse_read_reply(&Array(vec![bytes(b"x")])).is_err());
        let bad_section = Array(vec![Array(vec![bytes(b"s"), bytes(b"nope")])]);
        assert!(parse_read_reply(&bad_section).is_err());
    }

    #[test]
    fn dumps_payload_matches_json_dumps() {
        // Goldens probed from CPython `json.dumps(payload,
        // default=str)` (local python3, independent oracle).
        let payload = |pairs: Vec<(&str, Value)>| {
            let mut map = Map::new();
            for (key, value) in pairs {
                map.insert(key.to_string(), value);
            }
            map
        };
        // The fixture's own set/get vector, key order kept.
        let stored = dumps_payload(&payload(vec![
            ("status", json!("ok")),
            ("runner_id", json!("2b5e80b9-a171-4c81-b154-bdabe36dfa35")),
        ]));
        assert_eq!(
            stored,
            "{\"status\": \"ok\", \"runner_id\": \"2b5e80b9-a171-4c81-b154-bdabe36dfa35\"}"
        );
        let fx = fixture();
        assert_eq!(
            fx["command_result"]["redis"][0]["args"][1],
            json!(
                "'{\"status\": \"ok\", \"runner_id\": \"2b5e80b9-a171-4c81-b154-bdabe36dfa35\"}'"
            )
        );

        assert_eq!(
            dumps_payload(&payload(vec![
                ("a", json!([1, true, null])),
                ("b", json!({})),
            ])),
            "{\"a\": [1, true, null], \"b\": {}}"
        );
        // Control escapes shared with compact mode.
        assert_eq!(
            dumps_payload(&payload(vec![(
                "s",
                json!("a\"b\\c\nd\re\tf\x08g\x0ch\x00i\x1bj"),
            )])),
            "{\"s\": \"a\\\"b\\\\c\\nd\\re\\tf\\bg\\fh\\u0000i\\u001bj\"}"
        );
        // DEL + non-ASCII escaped (ensure_ascii), lowercase hex,
        // astral char as a surrogate pair.
        assert_eq!(
            dumps_payload(&payload(vec![("u", json!("~\u{7f}é\u{1d11e}"))])),
            "{\"u\": \"~\\u007f\\u00e9\\ud834\\udd1e\"}"
        );
        // Ints, negatives, floats incl. the repr tie-break and
        // exponent shapes.
        assert_eq!(
            dumps_payload(&payload(vec![
                ("n", json!(0)),
                ("neg", json!(-5)),
                ("f1", json!(1.5)),
                ("f2", json!(153838026194641.12)),
                ("f3", json!(1e300)),
                ("f4", json!(1.5e-7)),
                ("f5", json!(100000.0)),
                ("f6", json!(0.0001)),
                ("f7", json!(123456789.0)),
            ])),
            "{\"n\": 0, \"neg\": -5, \"f1\": 1.5, \"f2\": 153838026194641.12, \
             \"f3\": 1e+300, \"f4\": 1.5e-07, \"f5\": 100000.0, \"f6\": 0.0001, \
             \"f7\": 123456789.0}"
        );
        assert_eq!(
            dumps_payload(&payload(vec![
                ("empty", json!("")),
                ("nested", json!({"x": [1.25, "y"]})),
            ])),
            "{\"empty\": \"\", \"nested\": {\"x\": [1.25, \"y\"]}}"
        );
    }

    // -- scripted RESP mock: exact argv sequences -------------------

    const SKIPPED_SETUP: [&str; 4] = ["CLIENT", "HELLO", "AUTH", "SELECT"];

    fn read_command(reader: &mut BufReader<std::net::TcpStream>) -> Option<Vec<String>> {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            return None;
        }
        let count: usize = line.trim_start_matches('*').trim().parse().ok()?;
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            let mut header = String::new();
            reader.read_line(&mut header).ok()?;
            let len: usize = header.trim_start_matches('$').trim().parse().ok()?;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).ok()?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf).ok()?;
            args.push(String::from_utf8(buf).expect("mock argv is utf-8"));
        }
        Some(args)
    }

    const SCRIPT_EXHAUSTED: &[u8] = b"-ERR mock script exhausted\r\n";

    /// Serve sequential connections from `replies` in order (one
    /// connection per verb call), recording every non-setup
    /// command's argv. The server thread exits once the script is
    /// consumed and its connection closes, or on a 10s accept
    /// backstop (a test bug that connects too little still fails on
    /// its `recorded` assertions; one that sends too much gets
    /// connection errors).
    fn start_mock(replies: Vec<Vec<u8>>) -> (redis::Client, Arc<Mutex<Vec<Vec<String>>>>) {
        fn serve(
            stream: std::net::TcpStream,
            replies: &[Vec<u8>],
            recorded: &Arc<Mutex<Vec<Vec<String>>>>,
            index: &mut usize,
        ) {
            stream.set_nonblocking(false).expect("mock blocking stream");
            stream
                .set_read_timeout(Some(Duration::from_secs(15)))
                .expect("mock timeout");
            let mut reader = BufReader::new(stream.try_clone().expect("mock clone"));
            let mut writer = stream;
            while let Some(argv) = read_command(&mut reader) {
                if argv
                    .first()
                    .is_some_and(|cmd| SKIPPED_SETUP.contains(&cmd.as_str()))
                {
                    if writer.write_all(b"+OK\r\n").is_err() || writer.flush().is_err() {
                        break;
                    }
                    continue;
                }
                recorded.lock().expect("mock lock").push(argv);
                let reply = replies.get(*index).map_or(SCRIPT_EXHAUSTED, Vec::as_slice);
                *index += 1;
                if writer.write_all(reply).is_err() || writer.flush().is_err() {
                    break;
                }
            }
        }

        let listener = TcpListener::bind("127.0.0.1:0").expect("mock binds");
        let port = listener.local_addr().expect("mock addr").port();
        listener.set_nonblocking(true).expect("mock nonblocking");
        let recorded: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded_server = Arc::clone(&recorded);
        thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut index = 0usize;
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        serve(stream, &replies, &recorded_server, &mut index);
                        if index >= replies.len() {
                            return;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() > deadline {
                            return;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => return,
                }
            }
        });
        let client =
            redis::Client::open(format!("redis://127.0.0.1:{port}/")).expect("mock client parses");
        (client, recorded)
    }

    fn argv<const N: usize>(items: [&str; N]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn recorded(recorded: &Arc<Mutex<Vec<Vec<String>>>>) -> Vec<Vec<String>> {
        recorded.lock().expect("mock lock").clone()
    }

    fn resp_ok() -> Vec<u8> {
        b"+OK\r\n".to_vec()
    }

    fn resp_int(n: i64) -> Vec<u8> {
        format!(":{n}\r\n").into_bytes()
    }

    fn resp_str(s: &str) -> Vec<u8> {
        format!("${}\r\n{s}\r\n", s.len()).into_bytes()
    }

    fn resp_bytes(data: &[u8]) -> Vec<u8> {
        [
            format!("${}\r\n", data.len()).into_bytes(),
            data.to_vec(),
            b"\r\n".to_vec(),
        ]
        .concat()
    }

    fn resp_array(parts: Vec<Vec<u8>>) -> Vec<u8> {
        [format!("*{}\r\n", parts.len()).into_bytes(), parts.concat()].concat()
    }

    fn resp_err(msg: &str) -> Vec<u8> {
        format!("-{msg}\r\n").into_bytes()
    }

    // -- mock argv-sequence tests (FX-RSES-05 redis traces) ----------

    #[tokio::test]
    async fn mock_ensure_group_argv_and_busygroup() {
        let mid = fresh_id();
        let sk = keys::stream_key(&mid);
        let gn = keys::group_name(&mid);
        let expected = argv([
            "XGROUP",
            "CREATE",
            sk.as_str(),
            gn.as_str(),
            "$",
            "MKSTREAM",
        ]);

        let (client, log) = start_mock(vec![resp_ok()]);
        ensure_stream_group(Some(&client), &mid)
            .await
            .expect("ensure");
        assert_eq!(recorded(&log), vec![expected.clone()]);
        drop(client);

        // BUSYGROUP means the group exists: swallowed.
        let (client, log) = start_mock(vec![resp_err(
            "BUSYGROUP Consumer Group name already exists",
        )]);
        ensure_stream_group(Some(&client), &mid)
            .await
            .expect("busygroup swallowed");
        assert_eq!(recorded(&log), vec![expected]);
        drop(client);

        // Anything else propagates.
        let (client, _) = start_mock(vec![resp_err("NOGROUP missing")]);
        assert!(matches!(
            ensure_stream_group(Some(&client), &mid).await,
            Err(MachineOutboxError::Redis(_))
        ));
    }

    #[tokio::test]
    async fn mock_drain_offline_sequence() {
        let mid = fresh_id();
        let okey = keys::offline_stream_key(&mid);
        let sk = keys::stream_key(&mid);
        let gn = keys::group_name(&mid);

        // Empty buffer: XRANGE only, returns 0.
        let (client, log) = start_mock(vec![resp_array(vec![])]);
        assert_eq!(
            drain_offline_into_live(Some(&client), &mid)
                .await
                .expect("drain"),
            0
        );
        assert_eq!(
            recorded(&log),
            vec![argv(["XRANGE", okey.as_str(), "-", "+"])]
        );
        drop(client);

        // Two entries (the fixture's machine drain): XRANGE, XGROUP,
        // XADD, XADD (field order kept), DEL.
        let entry = |id: &str, mid_field: &str| {
            resp_array(vec![
                resp_str(id),
                resp_array(vec![
                    resp_str("mid"),
                    resp_str(mid_field),
                    resp_str("type"),
                    resp_str("ping"),
                    resp_str("payload"),
                    resp_str(&format!("{{\"type\": \"ping\", \"mid\": \"{mid_field}\"}}")),
                ]),
            ])
        };
        let xrange = resp_array(vec![
            entry("1790987270361-0", "e-moff-1"),
            entry("1790987270368-0", "4448f3a7-b821-450b-abf4-e8ff28ffd03a"),
        ]);
        let (client, log) = start_mock(vec![
            xrange,
            resp_ok(),
            resp_str("1790987270372-0"),
            resp_str("1790987270372-1"),
            resp_int(1),
        ]);
        assert_eq!(
            drain_offline_into_live(Some(&client), &mid)
                .await
                .expect("drain"),
            2
        );
        assert_eq!(
            recorded(&log),
            vec![
                argv(["XRANGE", okey.as_str(), "-", "+"]),
                argv([
                    "XGROUP",
                    "CREATE",
                    sk.as_str(),
                    gn.as_str(),
                    "$",
                    "MKSTREAM"
                ]),
                argv([
                    "XADD",
                    sk.as_str(),
                    "*",
                    "mid",
                    "e-moff-1",
                    "type",
                    "ping",
                    "payload",
                    "{\"type\": \"ping\", \"mid\": \"e-moff-1\"}"
                ]),
                argv([
                    "XADD",
                    sk.as_str(),
                    "*",
                    "mid",
                    "4448f3a7-b821-450b-abf4-e8ff28ffd03a",
                    "type",
                    "ping",
                    "payload",
                    "{\"type\": \"ping\", \"mid\": \"4448f3a7-b821-450b-abf4-e8ff28ffd03a\"}"
                ]),
                argv(["DEL", okey.as_str()]),
            ]
        );
        let fx = fixture();
        assert_eq!(fx["machine_drain_offline_into_live"]["moved"], json!(2));
        assert_eq!(
            fx["machine_drain_offline_into_live"]["buffer_exists_after"],
            json!(false)
        );
    }

    #[tokio::test]
    async fn mock_drain_bad_utf8_raises() {
        // Non-UTF-8 field bytes: Python's `.decode()` raises, so the
        // drain fails after the group call.
        let mid = fresh_id();
        let xrange = resp_array(vec![resp_array(vec![
            resp_str("9-0"),
            resp_array(vec![resp_bytes(b"mid"), resp_bytes(&[0xff, 0xfe])]),
        ])]);
        let (client, log) = start_mock(vec![xrange, resp_ok()]);
        let result = drain_offline_into_live(Some(&client), &mid).await;
        assert!(matches!(result, Err(MachineOutboxError::InvalidUtf8(_))));
        assert_eq!(recorded(&log).len(), 2, "fails after XRANGE + XGROUP");
    }

    #[tokio::test]
    async fn mock_read_decodes_fixture_vector() {
        // The fixture's `machine_read_for_session.gt` entry, served
        // raw: one ping with an empty-object payload.
        let mid = fresh_id();
        let sid = fresh_id();
        let sk = keys::stream_key(&mid);
        let gn = keys::group_name(&mid);
        let cn = keys::consumer_name(&sid);
        let reply = resp_array(vec![resp_array(vec![
            resp_str(&sk),
            resp_array(vec![resp_array(vec![
                resp_str("1790987273769-0"),
                resp_array(vec![
                    resp_str("mid"),
                    resp_str("mrd1"),
                    resp_str("type"),
                    resp_str("ping"),
                    resp_str("payload"),
                    resp_str("{}"),
                ]),
            ])]),
        ])]);
        let (client, log) = start_mock(vec![resp_ok(), reply]);
        let messages = read_for_session(Some(&client), &mid, &sid, 0, 100, false)
            .await
            .expect("read");
        assert_eq!(
            recorded(&log),
            vec![
                argv([
                    "XGROUP",
                    "CREATE",
                    sk.as_str(),
                    gn.as_str(),
                    "$",
                    "MKSTREAM"
                ]),
                argv([
                    "XREADGROUP",
                    "GROUP",
                    gn.as_str(),
                    cn.as_str(),
                    "COUNT",
                    "100",
                    "BLOCK",
                    "0",
                    "STREAMS",
                    sk.as_str(),
                    ">"
                ]),
            ]
        );
        let fx = fixture();
        let expected = fx["machine_read_for_session"]["gt"].as_array().expect("gt");
        assert_eq!(messages.len(), expected.len());
        for (message, want) in messages.iter().zip(expected.iter()) {
            assert_eq!(message.stream_id, want["stream_id"].as_str().expect("sid"));
            assert_eq!(message.mid, want["mid"].as_str().expect("mid"));
            assert_eq!(message.msg_type, want["type"].as_str().expect("type"));
            assert_eq!(message.body, want["body"]);
        }
        drop(client);

        // `use_zero` reads `0` instead of `>`.
        let (client, log) = start_mock(vec![resp_ok(), resp_array(vec![])]);
        let messages = read_for_session(Some(&client), &mid, &sid, 0, 100, true)
            .await
            .expect("read zero");
        assert!(messages.is_empty());
        assert_eq!(recorded(&log)[1].last().map(String::as_str), Some("0"));
        drop(client);

        // Transport failures log and return no entries; nil (block
        // timeout) reads as absent.
        let (client, _) = start_mock(vec![resp_ok(), resp_err("boom")]);
        assert!(read_for_session(Some(&client), &mid, &sid, 0, 100, false)
            .await
            .expect("read error")
            .is_empty());
        drop(client);
        let (client, _) = start_mock(vec![resp_ok(), b"$-1\r\n".to_vec()]);
        assert!(read_for_session(Some(&client), &mid, &sid, 0, 100, false)
            .await
            .expect("read nil")
            .is_empty());
    }

    #[tokio::test]
    async fn mock_ack_argv() {
        let mid = fresh_id();
        let sk = keys::stream_key(&mid);
        let gn = keys::group_name(&mid);
        let ids = ["1790987273769-0", "1790987273771-0"];
        let (client, log) = start_mock(vec![resp_int(2)]);
        let owned: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(
            ack_for_session(Some(&client), &mid, &owned)
                .await
                .expect("ack"),
            2
        );
        assert_eq!(
            recorded(&log),
            vec![argv([
                "XACK",
                sk.as_str(),
                gn.as_str(),
                "1790987273769-0",
                "1790987273771-0"
            ])]
        );
    }

    #[tokio::test]
    async fn mock_pel_marker_sequence() {
        // SET … EX 7200 (2x the 3600 default), EXISTS, DEL.
        let runner = runner_settings();
        assert_eq!(runner.access_token_ttl_secs, 3600);
        let sid = fresh_id();
        let key = keys::session_pel_drained_key(&sid);
        let (client, log) = start_mock(vec![resp_ok(), resp_int(1), resp_int(1)]);
        mark_pel_drained(Some(&client), &runner, &sid)
            .await
            .expect("mark");
        assert!(is_pel_drained(Some(&client), &sid)
            .await
            .expect("is drained"));
        clear_session_marker(Some(&client), &sid)
            .await
            .expect("clear");
        assert_eq!(
            recorded(&log),
            vec![
                argv(["SET", key.as_str(), "1", "EX", "7200"]),
                argv(["EXISTS", key.as_str()]),
                argv(["DEL", key.as_str()]),
            ]
        );
        let fx = fixture();
        assert_eq!(fx["machine_pel_marker"]["ttl"], json!(7200));
        assert_eq!(fx["machine_pel_marker"]["is_drained"], json!(true));
    }

    #[tokio::test]
    async fn mock_eviction_publish_bodies() {
        let mid = fresh_id();
        let channel = keys::session_eviction_channel(&mid);
        let (client, log) = start_mock(vec![resp_int(0), resp_int(0)]);
        publish_session_eviction(Some(&client), &mid, Some("old-sid"), "new-sid")
            .await
            .expect("publish");
        publish_session_eviction(Some(&client), &mid, None, "new-sid")
            .await
            .expect("publish none-old");
        assert_eq!(
            recorded(&log),
            vec![
                argv([
                    "PUBLISH",
                    channel.as_str(),
                    "{\"old_sid\": \"old-sid\", \"new_sid\": \"new-sid\"}"
                ]),
                argv([
                    "PUBLISH",
                    channel.as_str(),
                    "{\"old_sid\": null, \"new_sid\": \"new-sid\"}"
                ]),
            ]
        );
    }

    #[tokio::test]
    async fn mock_delete_stream_order() {
        // Live stream first, offline buffer second.
        let mid = fresh_id();
        let sk = keys::stream_key(&mid);
        let okey = keys::offline_stream_key(&mid);
        let (client, log) = start_mock(vec![resp_int(0), resp_int(0)]);
        delete_machine_stream(Some(&client), &mid)
            .await
            .expect("delete");
        assert_eq!(
            recorded(&log),
            vec![argv(["DEL", sk.as_str()]), argv(["DEL", okey.as_str()])]
        );
    }

    #[tokio::test]
    async fn mock_command_result_sequence() {
        // SET key <spaced JSON> EX 900, then GET present / missing /
        // corrupt (the fixture's `command_result` trace).
        let req = format!("req-{}", fresh_id());
        let key = keys::command_result_key(&req);
        let mut payload = Map::new();
        payload.insert("status".to_string(), Value::String("ok".to_string()));
        payload.insert(
            "runner_id".to_string(),
            Value::String("2b5e80b9-a171-4c81-b154-bdabe36dfa35".to_string()),
        );
        let (client, log) = start_mock(vec![
            resp_ok(),
            resp_str(
                "{\"status\": \"ok\", \"runner_id\": \"2b5e80b9-a171-4c81-b154-bdabe36dfa35\"}",
            ),
            b"$-1\r\n".to_vec(),
            resp_str("{corrupt"),
            resp_bytes(&[0xff, 0xfe]),
            resp_bytes(b""),
        ]);
        set_command_result(Some(&client), &req, &payload)
            .await
            .expect("set");
        let got = get_command_result(Some(&client), &req).await.expect("get");
        assert_eq!(
            got,
            Some(json!({
                "status": "ok",
                "runner_id": "2b5e80b9-a171-4c81-b154-bdabe36dfa35",
            }))
        );
        assert_eq!(
            get_command_result(Some(&client), &req)
                .await
                .expect("missing"),
            None
        );
        assert_eq!(
            get_command_result(Some(&client), &req)
                .await
                .expect("corrupt"),
            None
        );
        assert_eq!(
            get_command_result(Some(&client), &req)
                .await
                .expect("bad utf-8"),
            None,
            "undecodable bytes read as corrupt, never Err"
        );
        assert_eq!(
            get_command_result(Some(&client), &req)
                .await
                .expect("empty"),
            None,
            "empty values are falsy, like `if not raw`"
        );
        let fx = fixture();
        let trace = &fx["command_result"];
        assert_eq!(
            recorded(&log)[0],
            argv([
                "SET",
                key.as_str(),
                "{\"status\": \"ok\", \"runner_id\": \"2b5e80b9-a171-4c81-b154-bdabe36dfa35\"}",
                "EX",
                "900"
            ])
        );
        assert_eq!(recorded(&log)[1], argv(["GET", key.as_str()]));
        assert_eq!(trace["got"], got.expect("got"));
        assert!(trace["missing"].is_null());
        assert!(trace["corrupt"].is_null());
        assert_eq!(trace["ttl"], json!(900));
        assert_eq!(trace["expected_ttl"], json!(900));
        assert_eq!(trace["redis"][0]["kwargs"]["ex"], json!("900"));
    }

    // -- live roundtrips (127.0.0.1:6379, like `redis.rs`) ---------

    fn live_client() -> redis::Client {
        redis::Client::open("redis://127.0.0.1:6379/").expect("live redis parses")
    }

    async fn live_conn(client: &redis::Client) -> MultiplexedConnection {
        client
            .get_multiplexed_async_connection()
            .await
            .expect("live redis connects")
    }

    /// Delete a test's keys (best-effort order; every live test
    /// calls this).
    async fn cleanup(client: &redis::Client, dev_machine_id: &str, extra: &[&str]) {
        let mut conn = live_conn(client).await;
        let mut delete = redis::cmd("DEL");
        delete
            .arg(keys::stream_key(dev_machine_id))
            .arg(keys::offline_stream_key(dev_machine_id));
        for key in extra {
            delete.arg(key);
        }
        let _: () = delete.query_async(&mut conn).await.expect("cleanup del");
    }

    async fn raw_xadd(
        conn: &mut MultiplexedConnection,
        key: &str,
        id: &str,
        mid: &str,
        msg_type: &str,
        payload: &str,
    ) -> String {
        let mut xadd = redis::cmd("XADD");
        xadd.arg(key)
            .arg(id)
            .arg("mid")
            .arg(mid)
            .arg("type")
            .arg(msg_type)
            .arg("payload")
            .arg(payload);
        xadd.query_async(conn).await.expect("raw xadd")
    }

    async fn raw_xlen(conn: &mut MultiplexedConnection, key: &str) -> i64 {
        let mut xlen = redis::cmd("XLEN");
        xlen.arg(key);
        xlen.query_async(conn).await.expect("raw xlen")
    }

    async fn raw_exists(conn: &mut MultiplexedConnection, key: &str) -> i64 {
        let mut exists = redis::cmd("EXISTS");
        exists.arg(key);
        exists.query_async(conn).await.expect("raw exists")
    }

    fn result_payload(pairs: Vec<(&str, &str)>) -> Map<String, Value> {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.insert(key.to_string(), Value::String(value.to_string()));
        }
        map
    }

    #[tokio::test]
    async fn live_drain_moves_buffer_to_stream() {
        let client = live_client();
        let mid = fresh_id();
        let mut conn = live_conn(&client).await;
        raw_xadd(
            &mut conn,
            &keys::offline_stream_key(&mid),
            "*",
            "m-off-1",
            "ping",
            "{\"type\": \"ping\", \"mid\": \"m-off-1\"}",
        )
        .await;
        raw_xadd(
            &mut conn,
            &keys::offline_stream_key(&mid),
            "*",
            "m-off-2",
            "welcome",
            "{\"type\": \"welcome\", \"mid\": \"m-off-2\"}",
        )
        .await;

        let moved = drain_offline_into_live(Some(&client), &mid)
            .await
            .expect("drain");
        assert_eq!(moved, 2);
        assert_eq!(raw_xlen(&mut conn, &keys::stream_key(&mid)).await, 2);
        assert_eq!(
            raw_exists(&mut conn, &keys::offline_stream_key(&mid)).await,
            0,
            "buffer deleted after the move"
        );
        // Draining again moves nothing.
        assert_eq!(
            drain_offline_into_live(Some(&client), &mid)
                .await
                .expect("drain again"),
            0
        );
        cleanup(&client, &mid, &[]).await;
    }

    #[tokio::test]
    async fn live_read_on_missing_stream_creates_group() {
        let client = live_client();
        let mid = fresh_id();
        let sid = fresh_id();
        // A read creates the group (MKSTREAM) and finds nothing.
        assert!(read_for_session(Some(&client), &mid, &sid, 1, 100, false)
            .await
            .expect("read")
            .is_empty());
        let mut conn = live_conn(&client).await;
        assert_eq!(raw_exists(&mut conn, &keys::stream_key(&mid)).await, 1);
        cleanup(&client, &mid, &[]).await;
    }

    #[tokio::test]
    async fn live_read_ack_roundtrip() {
        let client = live_client();
        let mid = fresh_id();
        let sid = fresh_id();
        // The group must exist before seeding: `>` only sees
        // entries added after the group.
        ensure_stream_group(Some(&client), &mid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        raw_xadd(
            &mut conn,
            &keys::stream_key(&mid),
            "*",
            "mrd1",
            "ping",
            "{}",
        )
        .await;

        let messages = read_for_session(Some(&client), &mid, &sid, 1, 100, false)
            .await
            .expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].mid, "mrd1");
        assert_eq!(messages[0].msg_type, "ping");
        assert_eq!(messages[0].body, json!({}));
        let ids: Vec<String> = messages.iter().map(|m| m.stream_id.clone()).collect();
        assert_eq!(
            ack_for_session(Some(&client), &mid, &ids)
                .await
                .expect("ack"),
            1
        );
        assert_eq!(
            ack_for_session(Some(&client), &mid, &ids)
                .await
                .expect("ack again"),
            0
        );
        // PEL replay after the ack finds nothing (use_zero_n 0).
        assert!(read_for_session(Some(&client), &mid, &sid, 1, 100, true)
            .await
            .expect("read zero")
            .is_empty());
        let fx = fixture();
        assert_eq!(fx["machine_read_for_session"]["use_zero_n"], json!(0));
        cleanup(&client, &mid, &[]).await;
    }

    #[tokio::test]
    async fn live_markers_ttl_and_clear() {
        let client = live_client();
        let runner = runner_settings();
        let sid = fresh_id();
        mark_pel_drained(Some(&client), &runner, &sid)
            .await
            .expect("mark");
        assert!(is_pel_drained(Some(&client), &sid)
            .await
            .expect("is drained"));
        let mut conn = live_conn(&client).await;
        let mut ttl = redis::cmd("TTL");
        ttl.arg(keys::session_pel_drained_key(&sid));
        let ttl: i64 = ttl.query_async(&mut conn).await.expect("ttl");
        assert!(
            (7199..=7200).contains(&ttl),
            "ttl is 2x the 3600 default: {ttl}"
        );
        clear_session_marker(Some(&client), &sid)
            .await
            .expect("clear");
        assert!(!is_pel_drained(Some(&client), &sid)
            .await
            .expect("is drained after clear"));
        let fx = fixture();
        assert_eq!(fx["machine_pel_marker"]["ttl"], json!(7200));
        let marker = keys::session_pel_drained_key(&sid);
        cleanup(&client, &fresh_id(), &[&marker]).await;
    }

    #[tokio::test]
    async fn live_eviction_publish_payload() {
        use futures_util::StreamExt;

        let client = live_client();
        let mid = fresh_id();
        let channel = keys::session_eviction_channel(&mid);
        let mut pubsub = client.get_async_pubsub().await.expect("pubsub");
        pubsub.subscribe(&channel).await.expect("subscribe");
        publish_session_eviction(Some(&client), &mid, Some("old-sid"), "new-sid")
            .await
            .expect("publish");
        let message = pubsub.on_message().next().await.expect("published");
        assert_eq!(
            message.get_payload_bytes(),
            b"{\"old_sid\": \"old-sid\", \"new_sid\": \"new-sid\"}"
        );
    }

    #[tokio::test]
    async fn live_command_result_roundtrip() {
        let client = live_client();
        let req = format!("req-{}", fresh_id());
        let key = keys::command_result_key(&req);

        // Missing keys read as unknown.
        assert_eq!(
            get_command_result(Some(&client), &req)
                .await
                .expect("missing"),
            None
        );

        // The pending marker D-13 writes at enqueue time.
        set_command_result(
            Some(&client),
            &req,
            &result_payload(vec![
                ("status", "pending"),
                ("dev_machine_id", "12aad42f-f469-4813-bfe2-8847b93b0500"),
                ("requested_at", "2026-10-03T12:00:00+00:00"),
            ]),
        )
        .await
        .expect("set pending");
        assert_eq!(
            get_command_result(Some(&client), &req).await.expect("get"),
            Some(json!({
                "status": "pending",
                "dev_machine_id": "12aad42f-f469-4813-bfe2-8847b93b0500",
                "requested_at": "2026-10-03T12:00:00+00:00",
            }))
        );
        let mut conn = live_conn(&client).await;
        let mut ttl = redis::cmd("TTL");
        ttl.arg(&key);
        let ttl: i64 = ttl.query_async(&mut conn).await.expect("ttl");
        assert!((899..=900).contains(&ttl), "ttl is the 900 const: {ttl}");

        // The daemon write-back overwrites, and non-ASCII survives
        // the ensure_ascii roundtrip.
        set_command_result(
            Some(&client),
            &req,
            &result_payload(vec![
                ("status", "error"),
                ("dev_machine_id", "12aad42f-f469-4813-bfe2-8847b93b0500"),
                ("runner_id", ""),
                ("runner_name", ""),
                ("error", "débâcle\nretry"),
                ("reported_at", "2026-10-03T12:01:00+00:00"),
            ]),
        )
        .await
        .expect("set error");
        let got = get_command_result(Some(&client), &req)
            .await
            .expect("get")
            .expect("some");
        assert_eq!(got["status"], json!("error"));
        assert_eq!(got["error"], json!("débâcle\nretry"));

        // A corrupt value reads as unknown, never Err.
        let mut set = redis::cmd("SET");
        set.arg(&key).arg("{corrupt");
        let _: () = set.query_async(&mut conn).await.expect("corrupt");
        assert_eq!(
            get_command_result(Some(&client), &req)
                .await
                .expect("corrupt"),
            None
        );
        cleanup(&client, &fresh_id(), &[&key]).await;
    }

    #[tokio::test]
    async fn live_delete_drops_both_keys() {
        let client = live_client();
        let mid = fresh_id();
        ensure_stream_group(Some(&client), &mid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        raw_xadd(
            &mut conn,
            &keys::offline_stream_key(&mid),
            "*",
            "m",
            "ping",
            "{}",
        )
        .await;
        delete_machine_stream(Some(&client), &mid)
            .await
            .expect("delete");
        assert_eq!(raw_exists(&mut conn, &keys::stream_key(&mid)).await, 0);
        assert_eq!(
            raw_exists(&mut conn, &keys::offline_stream_key(&mid)).await,
            0
        );
    }

    // -- live Postgres (env-gated, like queries_git) ---------------

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        // Faithful `machine_session` shape (models COLUMNS + types);
        // the lookup only reads id/dev_machine_id/revoked_at/created_at.
        sqlx::query(
            "CREATE TEMPORARY TABLE machine_session (\
                id UUID PRIMARY KEY, \
                dev_machine_id UUID NOT NULL, \
                protocol_version INTEGER NOT NULL, \
                created_at TIMESTAMPTZ NOT NULL, \
                last_seen_at TIMESTAMPTZ NOT NULL, \
                revoked_at TIMESTAMPTZ NULL, \
                revoked_reason VARCHAR(32) NULL)",
        )
        .execute(&mut *tx)
        .await
        .expect("create temp table");
        tx
    }

    async fn seed_session(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        dev_machine_id: uuid::Uuid,
        revoked: bool,
    ) {
        sqlx::query(
            "INSERT INTO machine_session \
                (id, dev_machine_id, protocol_version, created_at, last_seen_at, revoked_at, revoked_reason) \
             VALUES ($1, $2, 4, now(), now(), CASE WHEN $3 THEN now() ELSE NULL END, \
                CASE WHEN $3 THEN 'evicted' ELSE NULL END)",
        )
        .bind(id)
        .bind(dev_machine_id)
        .bind(revoked)
        .execute(&mut **tx)
        .await
        .expect("seed session");
    }

    #[tokio::test]
    async fn live_active_session_id_lookup() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let mid = uuid::Uuid::new_v4();
        assert_eq!(
            active_session_id_for_machine(&mut *tx, mid)
                .await
                .expect("lookup"),
            None
        );
        let active = uuid::Uuid::new_v4();
        seed_session(&mut tx, uuid::Uuid::new_v4(), mid, true).await;
        seed_session(&mut tx, active, mid, false).await;
        assert_eq!(
            active_session_id_for_machine(&mut *tx, mid)
                .await
                .expect("lookup"),
            Some(active)
        );
        // A revoked-only machine reads as offline.
        let ghost = uuid::Uuid::new_v4();
        seed_session(&mut tx, uuid::Uuid::new_v4(), ghost, true).await;
        assert_eq!(
            active_session_id_for_machine(&mut *tx, ghost)
                .await
                .expect("lookup"),
            None
        );
    }

    #[tokio::test]
    async fn live_enqueue_branches() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let client = live_client();
        let runner = runner_settings();
        assert_eq!(runner.offline_stream_maxlen, 1000);
        assert_eq!(runner.offline_stream_ttl_secs, 86400);
        let mut tx = live_tx(&pool).await;

        // Live branch: an active session row routes to the stream.
        let live_mid = uuid::Uuid::new_v4();
        seed_session(&mut tx, uuid::Uuid::new_v4(), live_mid, false).await;
        let stream_id =
            enqueue_for_machine(Some(&client), &mut *tx, &runner, live_mid, &message("ping"))
                .await
                .expect("enqueue live")
                .expect("live returns a stream id");
        assert!(stream_id.contains('-'), "stream id shape: {stream_id}");
        let mut conn = live_conn(&client).await;
        assert_eq!(
            raw_xlen(&mut conn, &keys::stream_key(&live_mid.to_string())).await,
            1
        );
        let live_key = keys::stream_key(&live_mid.to_string());

        // Offline-buffer branch: no row, queueable type.
        let off_mid = uuid::Uuid::new_v4();
        let buffered =
            enqueue_for_machine(Some(&client), &mut *tx, &runner, off_mid, &message("ping"))
                .await
                .expect("enqueue offline");
        assert_eq!(buffered, None);
        let off_key = keys::offline_stream_key(&off_mid.to_string());
        assert_eq!(raw_xlen(&mut conn, &off_key).await, 1);
        let mut ttl = redis::cmd("TTL");
        ttl.arg(&off_key);
        let ttl: i64 = ttl.query_async(&mut conn).await.expect("ttl");
        assert!(
            (86399..=86400).contains(&ttl),
            "buffer ttl is the 86400 default: {ttl}"
        );

        // Offline-reject branch: no row, `create_runner` raises with
        // the exact `MachineOfflineError` text.
        let error = enqueue_for_machine(
            Some(&client),
            &mut *tx,
            &runner,
            off_mid,
            &message("create_runner"),
        )
        .await
        .expect_err("create_runner rejects offline");
        assert_eq!(
            error.to_string(),
            format!("dev machine {off_mid} is offline; type 'create_runner' cannot queue")
        );

        // Unknown types raise before any I/O even with live remotes.
        let error = enqueue_for_machine(
            Some(&client),
            &mut *tx,
            &runner,
            live_mid,
            &message("assign"),
        )
        .await
        .expect_err("unknown type raises");
        assert_eq!(error.to_string(), "unknown machine message type 'assign'");

        // Reject/unknown paths enqueue nothing.
        assert_eq!(raw_xlen(&mut conn, &live_key).await, 1);
        assert_eq!(raw_xlen(&mut conn, &off_key).await, 1);
        cleanup(&client, &live_mid.to_string(), &[]).await;
        cleanup(&client, &off_mid.to_string(), &[]).await;
    }
}
