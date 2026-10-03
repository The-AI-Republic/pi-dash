//! Per-runner Redis Streams outbox verbs (D-14, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/services/outbox.py:139-737` (the
//! Redis operations; key builders, `_serialize`, `_decode_read_result`
//! and the stream-id arithmetic live in
//! `pidash_types::runner_sessions`, the session-row SQL in
//! [`super::models`]).
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
//! same defaults, including the `[1, 55]` long-poll clamp): the
//! offline `maxlen` / TTL for [`enqueue_for_runner`], 2x
//! `access_token_ttl_secs` for [`mark_pel_drained`] and
//! [`schedule_stream_cleanup_for_runner`], and
//! `long_poll_interval_secs` for [`reap_idle_consumers`]' default
//! idle floor. `RUNNER_STREAM_CONSUMER_IDLE_MS` has no Django-side
//! entry (it is a `getattr` default in `outbox.py:402`), so it stays
//! a per-call override there.
//!
//! IDs are `&str` (mirroring Python's `UUID | str` unions), except
//! [`active_session_id_for_runner`] and [`enqueue_for_runner`],
//! which bind a typed [`uuid::Uuid`] to the session lookup. The
//! sweeper chain (`due_runners_for_stream_cleanup` →
//! `delete_runner_stream` / `remove_stream_cleanup_marker`) stays
//! total: member strings pass through without parsing.
//!
//! All commands are raw `redis::cmd` spells (the `streams`
//! `AsyncCommands` helpers are feature-gated off): the argv order
//! matches redis-py 5.0.4 byte for byte (`XADD … MAXLEN ~ N * …`,
//! `XTRIM … MINID ~ id`, `XREADGROUP … COUNT n BLOCK ms STREAMS …`,
//! `XAUTOCLAIM … COUNT 200 JUSTID`), pinned by the scripted-server
//! tests below.
//!
//! # Failure policy
//!
//! Each verb mirrors its Python `try`/`except` exactly: transport
//! failures swallowed in Python (with `logger.exception`) return the
//! same value here (logged with `tracing::warn!`); failures that
//! propagate in Python surface as [`OutboxError`]:
//!
//! | verb | `None` client | transport error | reply-shape error |
//! | ---- | ------------- | --------------- | ----------------- |
//! | `ensure_stream_group` | `Ok(())` | BUSYGROUP → `Ok(())`, else `Err` | — |
//! | `active_session_id_for_runner` | — (SQL) | `Err(Db)` | — |
//! | `enqueue_for_runner` | warn + `Ok(None)` | `Err` | — |
//! | `drain_offline_into_live` | `Ok(0)` | `Err` | `Err(InvalidUtf8)` |
//! | `claim_pending_for_new_session` | `0` | log + claimed so far | log + claimed so far |
//! | `delete_consumer` | `0` | log + `0` | log + `0` (`int()` is inside the `try`) |
//! | `reap_idle_consumers` | `Ok(0)` | `XINFO` → log + `Ok(0)` | `Err` (`int()`/`.decode()` are outside the `try`) |
//! | `mark_pel_drained` / `clear_session_marker` | `Ok(())` | `Err` | — |
//! | `is_pel_drained` | `Ok(false)` | `Err` | — |
//! | `read_for_session` | `Ok(vec![])` | ensure → `Err`; `XREADGROUP` → log + `Ok(vec![])` | `Err` (decode is outside the `try`) |
//! | `ack_for_session` | `Ok(0)` | `Err` | — |
//! | `publish_session_eviction` | `Ok(())` | `Err` | — |
//! | `schedule_stream_cleanup_for_runner` | `Ok(())` | `Err` | — |
//! | `due_runners_for_stream_cleanup` | `Ok(vec![])` | `Err` | `Err(InvalidUtf8)` |
//! | `remove_stream_cleanup_marker` | `Ok(())` | `Err` | — |
//! | `delete_runner_stream` | `Ok(())` | `Err` | — |
//! | `safe_trim_runner_stream` | `Ok(None)` | log + `Ok(None)` | `Err` only for undecodable ids; anything else → `Ok(None)` |
//!
//! # Collapsed twins
//!
//! `read_for_session` / `aread_for_session` collapse into the single
//! async [`read_for_session`]: Rust has no sync Redis client here,
//! and both twins issue the identical command.
//!
//! # Ported bugs (also listed in the PR)
//!
//! * `claim_pending_for_new_session` never terminates on a non-empty
//!   PEL in Python: `justid=True` makes redis-py return
//!   `response[1]` (the flat id list), discarding the cursor, so
//!   the loop mistakes `result[0]` (a stream id) for the cursor and
//!   re-issues `XAUTOCLAIM` from the same id forever while
//!   accumulating `len()` of id strings as the count
//!   (TRACE.md bug 3; `claim_pending_nonempty_spins`: 18721
//!   iterations in 1.5s, never returns; production pins
//!   redis==5.0.4 so this is live). A line-by-line port would wedge
//!   the server the same way the Python hang wedges the HTTP
//!   worker, so the Rust loop reads the cursor the server actually
//!   returns, paginates to `0-0`, and returns the claimed count.
//!   Every *terminating* Python trace still replays exactly: empty
//!   PEL returns `0` with no `DELCONSUMER` (the old consumer is
//!   kept — `claim_pending_empty_pel`), transport errors return the
//!   count so far, `None` returns `0`. The non-empty-PEL
//!   behavior (spin vs terminate-and-claim) is the one known
//!   divergence, and the `delete_consumer`-after-`0-0` branch —
//!   unreachable in Python — becomes reachable here.
//! * `claim_pending_for_new_session` runs even when `old_consumer`
//!   is `None` (`outbox.py:297-305` — deliberate: `XAUTOCLAIM`
//!   scans the whole group PEL; the fixture's `None`-consumer spin
//!   proves it). Kept.
//! * `reap_idle_consumers` returns the sum of removed consumers'
//!   *pending counts*, not the number of consumers removed
//!   (`outbox.py:424` sums `XGROUP DELCONSUMER` results; TRACE.md
//!   bug 4). Kept.
//! * `logger.exception`-then-continue swallows in `claim_pending`
//!   (`xautoclaim`), `delete_consumer`, `reap_idle_consumers`
//!   (`xinfo`), `read_for_session` (`xreadgroup`) and
//!   `safe_trim_runner_stream` (`xinfo`/`xpending`/`xtrim`) return
//!   the defaults above instead of raising. Kept.
//! * `safe_trim_runner_stream` sends `MINID ~` (redis-py's
//!   `approximate=True` default), so single-node streams trim `0`
//!   and bulk trims drop whole nodes only (TRACE.md FX-RSES-05
//!   note). Kept.
//!
//! Fixture replayed by the tests below:
//! `rust-api/fixtures/runner_sessions/fx-rses-05-outbox-ops.json`
//! (FX-RSES-05, runner sections; machine sections are
//! PIDASHCONV-551's).

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use redis::aio::MultiplexedConnection;
use serde_json::{Map, Value};
use thiserror::Error as ThisError;

use pidash_types::runner_sessions::keys::runner as keys;
use pidash_types::runner_sessions::{
    decode_read_result, decrement_stream_id, eviction_body, is_offline_reject_runner_type,
    is_valid_runner_type, min_stream_id, serialize, DecodeError, DecodedMessage, StreamEntry,
    StreamRead,
};

use super::models::runner_session;
use crate::config::RunnerSettings;

/// `XAUTOCLAIM … COUNT` page size (`outbox.py:322`).
const CLAIM_PAGE_COUNT: i64 = 200;

/// Failures that propagate out of the outbox verbs — exactly the
/// failures Python lets raise (see the module failure-policy table).
/// Everything Python swallows with `logger.exception` stays a plain
/// return value, never one of these.
#[derive(Debug, ThisError)]
pub enum OutboxError {
    /// `ValueError: unknown message type …` from
    /// [`enqueue_for_runner`] (`outbox.py:234`).
    #[error("unknown message type '{0}'")]
    UnknownMessageType(String),

    /// [`enqueue_for_runner`] for an offline runner with a
    /// non-queueable type (`outbox.py:246`). The message matches
    /// `RunnerOfflineError` verbatim.
    #[error("runner {runner_id} is offline; type '{message_type}' cannot queue")]
    RunnerOffline {
        runner_id: String,
        message_type: String,
    },

    /// A Redis transport failure Python lets propagate (every verb
    /// without a `try` around the command, plus non-`BUSYGROUP`
    /// group creation).
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    /// A session-lookup SQL failure (`outbox.py:203-214` has no
    /// `try`).
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    /// `read_for_session` payload decode failure: invalid UTF-8 or a
    /// non-object payload with missing `mid`/`type`
    /// (`_decode_read_result`'s `.decode()` calls sit outside its
    /// `try`, `outbox.py:164-189`).
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),

    /// Non-UTF-8 bytes where Python's `.decode()` would raise: drain
    /// field bytes (`outbox.py:274-279`), cleanup-zset members
    /// (`:589-591`), consumer names (`:418-419`), group names /
    /// last-delivered / min-pending ids in the trim (`:636-642`,
    /// `:658-667`).
    #[error("invalid utf-8: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// A reply shape a real server never sends, where Python would
    /// raise `TypeError`/`ValueError`/`AttributeError` out of the
    /// parsing code (non-array `XREADGROUP`/`XINFO`/`XPENDING`
    /// replies, non-integer consumer stats, unpack failures).
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

/// `int(time.time())` (`outbox.py:577,587`).
fn epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Scan flat `XINFO` pairs for `key`, returning the raw value.
/// Entries that are not flat arrays behave like Python's
/// `_consumer_info_value` dict-conversion failure: the key reads as
/// missing.
fn info_field<'a>(entry: &'a redis::Value, key: &str) -> Option<&'a redis::Value> {
    let flat = as_array(entry)?;
    for pair in flat.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        if as_bytes(&pair[0]) == Some(key.as_bytes()) {
            return Some(&pair[1]);
        }
    }
    None
}

/// Open a multiplexed connection for one verb call (Python issues a
/// verb's commands over its single client).
async fn connect(client: &redis::Client) -> Result<MultiplexedConnection, redis::RedisError> {
    client.get_multiplexed_async_connection().await
}

/// Create the persistent stream + consumer group if missing
/// (`outbox.py:139-161`). `BUSYGROUP` means the group exists.
/// The sync/async twins collapse: one async body serves all verbs.
async fn ensure_group(
    connection: &mut MultiplexedConnection,
    runner_id: &str,
) -> Result<(), redis::RedisError> {
    let mut create = redis::cmd("XGROUP");
    create
        .arg("CREATE")
        .arg(keys::stream_key(runner_id))
        .arg(keys::group_name(runner_id))
        .arg("$")
        .arg("MKSTREAM");
    let created: Result<(), redis::RedisError> = create.query_async(connection).await;
    match created {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("BUSYGROUP") => Ok(()),
        Err(error) => Err(error),
    }
}

/// Idempotent `XGROUP CREATE … MKSTREAM` for the runner stream
/// (`outbox.py:192-197`).
pub async fn ensure_stream_group(
    client: Option<&redis::Client>,
    runner_id: &str,
) -> Result<(), OutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    ensure_group(&mut connection, runner_id).await?;
    Ok(())
}

/// The active session id for a runner, or `None`
/// (`outbox.py:203-214`). Python returns `str(sid)`; the typed
/// [`uuid::Uuid`] renders identically and every caller only tests
/// presence.
pub async fn active_session_id_for_runner<'e, E>(
    ex: E,
    runner_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, OutboxError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id: Option<uuid::Uuid> = sqlx::query_scalar(runner_session::ACTIVE_ID_SQL)
        .bind(runner_id)
        .fetch_optional(ex)
        .await?;
    Ok(id)
}

/// Enqueue a control message (`outbox.py:220-253`).
///
/// Returns the live-stream id when a session is active, `None` when
/// buffered offline or when `client` is `None` (warn-logged, before
/// the session lookup — no SQL runs then). Raises
/// [`OutboxError::UnknownMessageType`] for types outside
/// `_VALID_TYPES` (checked first, even with no client) and
/// [`OutboxError::RunnerOffline`] for offline-reject types with no
/// active session.
pub async fn enqueue_for_runner<'e, E>(
    client: Option<&redis::Client>,
    ex: E,
    runner: &RunnerSettings,
    runner_id: uuid::Uuid,
    message: &Map<String, Value>,
) -> Result<Option<String>, OutboxError>
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
    if !is_valid_runner_type(&fields.msg_type) {
        return Err(OutboxError::UnknownMessageType(fields.msg_type));
    }
    let Some(client) = client else {
        tracing::warn!(
            runner_id = %runner_id,
            "redis unavailable; cannot enqueue for runner"
        );
        return Ok(None);
    };
    let rid = runner_id.to_string();

    if active_session_id_for_runner(ex, runner_id).await?.is_some() {
        let mut connection = connect(client).await?;
        ensure_group(&mut connection, &rid).await?;
        let mut xadd = redis::cmd("XADD");
        xadd.arg(keys::stream_key(&rid))
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

    if is_offline_reject_runner_type(&fields.msg_type) {
        return Err(OutboxError::RunnerOffline {
            runner_id: rid,
            message_type: fields.msg_type,
        });
    }

    let mut connection = connect(client).await?;
    let key = keys::offline_stream_key(&rid);
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
/// (`outbox.py:256-283`). Returns the number of entries moved.
/// Field bytes round-trip through UTF-8 exactly like Python's
/// decode-then-`XADD` (non-UTF-8 raises there, `Err` here).
pub async fn drain_offline_into_live(
    client: Option<&redis::Client>,
    runner_id: &str,
) -> Result<usize, OutboxError> {
    let Some(client) = client else {
        return Ok(0);
    };
    let mut connection = connect(client).await?;
    let okey = keys::offline_stream_key(runner_id);
    let mut xrange = redis::cmd("XRANGE");
    xrange.arg(&okey).arg("-").arg("+");
    let reply: redis::Value = xrange.query_async(&mut connection).await?;
    let entries = match &reply {
        redis::Value::Nil => &[][..],
        redis::Value::Array(items) => &items[..],
        _ => {
            return Err(OutboxError::UnexpectedReply("XRANGE reply not an array"));
        }
    };
    if entries.is_empty() {
        return Ok(0);
    }
    ensure_group(&mut connection, runner_id).await?;
    let sk = keys::stream_key(runner_id);
    let mut moved = 0usize;
    for entry in entries {
        // `for _, fields in entries`: a non-pair entry fails to
        // unpack in Python, `Err` here.
        let pair = match as_array(entry) {
            Some(pair) if pair.len() == 2 => pair,
            _ => {
                return Err(OutboxError::UnexpectedReply(
                    "XRANGE entry not an id/fields pair",
                ));
            }
        };
        let flat = match as_array(&pair[1]) {
            Some(flat) => flat,
            None => {
                return Err(OutboxError::UnexpectedReply("XRANGE fields not an array"));
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
                    return Err(OutboxError::UnexpectedReply("XRANGE field name not bytes"));
                }
            };
            let value = match as_bytes(&pair[1]) {
                Some(value) => std::str::from_utf8(value)?,
                None => {
                    return Err(OutboxError::UnexpectedReply("XRANGE field value not bytes"));
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

/// Split an `XAUTOCLAIM … JUSTID` reply into its cursor and claimed
/// ids. redis-py discards the cursor (`parse_xautoclaim` returns
/// `response[1]`); the raw reply keeps it, which is what lets the
/// loop below terminate (see the ported-bugs note).
fn parse_autoclaim(reply: &redis::Value) -> Option<(String, Vec<Vec<u8>>)> {
    let parts = as_array(reply)?;
    if parts.is_empty() {
        return None;
    }
    let cursor = std::str::from_utf8(as_bytes(&parts[0])?).ok()?.to_string();
    // `result[1] if len(result) > 1 else []`, plus tolerance for the
    // Redis 7 third (deleted-ids) element, which Python never sees.
    let ids = if parts.len() > 1 {
        let raw = as_array(&parts[1])?;
        let mut ids = Vec::with_capacity(raw.len());
        for id in raw {
            ids.push(as_bytes(id)?.to_vec());
        }
        ids
    } else {
        Vec::new()
    };
    Some((cursor, ids))
}

/// Reassign every pending PEL entry in the group to `new_consumer`
/// (`outbox.py:286-340`). Paginated `XAUTOCLAIM` loop; returns the
/// number of stream ids claimed.
///
/// Runs even when `old_consumer` is `None` (the whole-group scan is
/// the point — `outbox.py:297-305`); the old consumer entry is
/// deleted only after the cursor reaches `0-0`, and only when an
/// old consumer was named. An empty claim page returns the count so
/// far *without* deleting (Python's `if not result: return
/// claimed`), so an empty PEL keeps the old consumer. Transport or
/// shape failures log and return the count so far, never `Err`.
pub async fn claim_pending_for_new_session(
    client: Option<&redis::Client>,
    runner_id: &str,
    old_consumer: Option<&str>,
    new_consumer: &str,
    min_idle_ms: i64,
) -> usize {
    let Some(client) = client else {
        return 0;
    };
    let sk = keys::stream_key(runner_id);
    let gn = keys::group_name(runner_id);
    let mut connection = match connect(client).await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xautoclaim failed for runner");
            return 0;
        }
    };
    let mut cursor = "0-0".to_string();
    let mut claimed = 0usize;
    loop {
        let mut autoclaim = redis::cmd("XAUTOCLAIM");
        autoclaim
            .arg(&sk)
            .arg(&gn)
            .arg(new_consumer)
            .arg(min_idle_ms)
            .arg(&cursor)
            .arg("COUNT")
            .arg(CLAIM_PAGE_COUNT)
            .arg("JUSTID");
        let reply: redis::Value = match autoclaim.query_async(&mut connection).await {
            Ok(reply) => reply,
            Err(error) => {
                tracing::warn!(%error, runner_id, "xautoclaim failed for runner");
                return claimed;
            }
        };
        let (next_cursor, ids) = match parse_autoclaim(&reply) {
            Some(parsed) => parsed,
            None => {
                tracing::warn!(
                    runner_id,
                    "xautoclaim failed for runner: unexpected reply shape"
                );
                return claimed;
            }
        };
        if ids.is_empty() {
            return claimed;
        }
        claimed += ids.len();
        if next_cursor == "0-0" {
            if let Some(old) = old_consumer.filter(|name| !name.is_empty()) {
                delete_consumer_with(&mut connection, runner_id, old).await;
            }
            return claimed;
        }
        cursor = next_cursor;
    }
}

/// `XGROUP DELCONSUMER` over an open connection, swallowing every
/// failure to `0` (`outbox.py:355-369` — the `int()` sits inside
/// the `try`, so even an unparsable reply logs and returns `0`).
async fn delete_consumer_with(
    connection: &mut MultiplexedConnection,
    runner_id: &str,
    consumer: &str,
) -> usize {
    let mut delconsumer = redis::cmd("XGROUP");
    delconsumer
        .arg("DELCONSUMER")
        .arg(keys::stream_key(runner_id))
        .arg(keys::group_name(runner_id))
        .arg(consumer);
    let removed: Result<i64, redis::RedisError> = delconsumer.query_async(connection).await;
    match removed {
        Ok(count) => usize::try_from(count).unwrap_or(0),
        Err(error) => {
            tracing::warn!(
                %error,
                runner_id,
                consumer,
                "xgroup delconsumer failed for runner consumer"
            );
            0
        }
    }
}

/// Delete an idle Redis stream consumer after its PEL has been
/// handed off (`outbox.py:343-369`). A falsy consumer returns `0`
/// with no Redis call.
pub async fn delete_consumer(
    client: Option<&redis::Client>,
    runner_id: &str,
    consumer: Option<&str>,
) -> usize {
    let Some(name) = consumer.filter(|name| !name.is_empty()) else {
        return 0;
    };
    let Some(client) = client else {
        return 0;
    };
    let mut connection = match connect(client).await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(
                %error,
                runner_id,
                consumer = name,
                "xgroup delconsumer failed for runner consumer"
            );
            return 0;
        }
    };
    delete_consumer_with(&mut connection, runner_id, name).await
}

/// Consumer `name` from an `XINFO CONSUMERS` entry
/// (`outbox.py:415-421`). Missing reads as absent
/// (`_consumer_info_value`'s `None` default → the entry is
/// skipped); undecodable bytes raise in Python (`Err` here).
fn consumer_name(entry: &redis::Value) -> Result<Option<String>, OutboxError> {
    let value = match info_field(entry, "name") {
        Some(value) => value,
        None => return Ok(None),
    };
    match value {
        redis::Value::BulkString(bytes) => Ok(Some(std::str::from_utf8(bytes)?.to_string())),
        redis::Value::SimpleString(text) => Ok(Some(text.clone())),
        // `0` is falsy (`if not name` skips it); other integers
        // proceed to the keep/pending/idle checks.
        redis::Value::Int(0) => Ok(None),
        redis::Value::Int(count) => Ok(Some(count.to_string())),
        redis::Value::Boolean(true) => Ok(Some("True".to_string())),
        // `False`/`nil`/exotic: falsy (skipped) or unreachable from
        // a real server (nothing useful to delete by).
        _ => Ok(None),
    }
}

/// Consumer `pending`/`idle` stat from an `XINFO CONSUMERS` entry
/// (`outbox.py:416-417,420-422`, `int(… or 0)`). Missing or
/// empty reads as `0`; anything `int()` rejects raises in Python
/// (`Err` here).
fn consumer_stat(entry: &redis::Value, key: &'static str) -> Result<i64, OutboxError> {
    let unexpected = || OutboxError::UnexpectedReply("consumer pending/idle not an integer");
    let value = match info_field(entry, key) {
        Some(value) => value,
        None => return Ok(0),
    };
    // `int(… or 0)`: falsy reads as 0, integers pass through,
    // numeric bytes parse (Python's `int()` strips whitespace).
    match value {
        redis::Value::Int(count) => Ok(*count),
        redis::Value::Boolean(flag) => Ok(i64::from(*flag)),
        redis::Value::Nil => Ok(0),
        redis::Value::BulkString(bytes) => match std::str::from_utf8(bytes) {
            Ok("") => Ok(0),
            Ok(text) => text.trim().parse::<i64>().map_err(|_| unexpected()),
            Err(_) => Err(unexpected()),
        },
        redis::Value::SimpleString(text) => {
            if text.is_empty() {
                return Ok(0);
            }
            text.trim().parse::<i64>().map_err(|_| unexpected())
        }
        redis::Value::Double(count) => Ok(*count as i64),
        _ => Err(unexpected()),
    }
}

/// Drop old zero-pending Redis stream consumers for one runner
/// (`outbox.py:382-425`). Returns the sum of removed consumers'
/// *pending counts* (each `XGROUP DELCONSUMER` reports its
/// consumer's pending entries), not the number removed — reaping a
/// zero-pending consumer returns `0` while still deleting it.
///
/// `min_idle_ms` overrides the default floor of
/// `max(long_poll_interval_secs * 4 * 1000, 120_000)`.
pub async fn reap_idle_consumers(
    client: Option<&redis::Client>,
    runner: &RunnerSettings,
    runner_id: &str,
    keep_consumers: &HashSet<String>,
    min_idle_ms: Option<i64>,
) -> Result<usize, OutboxError> {
    let Some(client) = client else {
        return Ok(0);
    };
    let floor =
        min_idle_ms.unwrap_or_else(|| (runner.long_poll_interval_secs * 4 * 1000).max(120_000));
    let mut connection = match connect(client).await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xinfo consumers failed for runner");
            return Ok(0);
        }
    };
    let mut xinfo = redis::cmd("XINFO");
    xinfo
        .arg("CONSUMERS")
        .arg(keys::stream_key(runner_id))
        .arg(keys::group_name(runner_id));
    let reply: redis::Value = match xinfo.query_async(&mut connection).await {
        Ok(reply) => reply,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xinfo consumers failed for runner");
            return Ok(0);
        }
    };
    let entries = match &reply {
        redis::Value::Nil => &[][..],
        redis::Value::Array(entries) => &entries[..],
        _ => {
            return Err(OutboxError::UnexpectedReply(
                "XINFO CONSUMERS reply not an array",
            ));
        }
    };
    let mut removed = 0usize;
    for entry in entries {
        let name = match consumer_name(entry)? {
            Some(name) if !name.is_empty() && !keep_consumers.contains(&name) => name,
            _ => continue,
        };
        if consumer_stat(entry, "pending")? > 0 {
            continue;
        }
        if consumer_stat(entry, "idle")? < floor {
            continue;
        }
        removed += delete_consumer_with(&mut connection, runner_id, &name).await;
    }
    Ok(removed)
}

/// Convert an `XREADGROUP` reply into the pre-decode sections
/// (`outbox.py:164-189` input shape). `nil` (block timeout, no
/// entries) reads as absent, like Python's `if not result`.
fn parse_read_reply(reply: &redis::Value) -> Result<Option<Vec<StreamRead>>, OutboxError> {
    match reply {
        redis::Value::Nil => Ok(None),
        redis::Value::Array(streams) => {
            let mut out = Vec::with_capacity(streams.len());
            for stream in streams {
                // `for _, entries in result`.
                let section = match as_array(stream) {
                    Some(section) if section.len() == 2 => section,
                    _ => {
                        return Err(OutboxError::UnexpectedReply(
                            "XREADGROUP stream section not a name/entries pair",
                        ));
                    }
                };
                let name = match as_bytes(&section[0]) {
                    Some(name) => name.to_vec(),
                    None => {
                        return Err(OutboxError::UnexpectedReply(
                            "XREADGROUP stream name not bytes",
                        ));
                    }
                };
                let raw_entries = match as_array(&section[1]) {
                    Some(entries) => entries,
                    None => {
                        return Err(OutboxError::UnexpectedReply(
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
                            return Err(OutboxError::UnexpectedReply(
                                "XREADGROUP entry not an id/fields pair",
                            ));
                        }
                    };
                    let id = match as_bytes(&pair[0]) {
                        Some(id) => id.to_vec(),
                        None => {
                            return Err(OutboxError::UnexpectedReply(
                                "XREADGROUP entry id not bytes",
                            ));
                        }
                    };
                    let flat = match as_array(&pair[1]) {
                        Some(flat) => flat,
                        None => {
                            return Err(OutboxError::UnexpectedReply(
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
                                return Err(OutboxError::UnexpectedReply(
                                    "XREADGROUP field name not bytes",
                                ));
                            }
                        };
                        let value = match as_bytes(&pair[1]) {
                            Some(value) => value.to_vec(),
                            None => {
                                return Err(OutboxError::UnexpectedReply(
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
        _ => Err(OutboxError::UnexpectedReply(
            "XREADGROUP reply neither nil nor array",
        )),
    }
}

/// One `XREADGROUP` against the runner stream
/// (`outbox.py:452-523`, sync and async twins collapsed).
/// `use_zero` reads `0` (the consumer's PEL replay), otherwise `>`
/// (new entries). Transport failures log and return no entries;
/// decode failures propagate.
pub async fn read_for_session(
    client: Option<&redis::Client>,
    runner_id: &str,
    session_id: &str,
    block_ms: i64,
    count: i64,
    use_zero: bool,
) -> Result<Vec<DecodedMessage>, OutboxError> {
    let Some(client) = client else {
        return Ok(Vec::new());
    };
    let mut connection = connect(client).await?;
    ensure_group(&mut connection, runner_id).await?;
    let last_id = if use_zero { "0" } else { ">" };
    let mut xreadgroup = redis::cmd("XREADGROUP");
    xreadgroup
        .arg("GROUP")
        .arg(keys::group_name(runner_id))
        .arg(keys::consumer_name(session_id))
        .arg("COUNT")
        .arg(count)
        .arg("BLOCK")
        .arg(block_ms)
        .arg("STREAMS")
        .arg(keys::stream_key(runner_id))
        .arg(last_id);
    let reply: redis::Value = match xreadgroup.query_async(&mut connection).await {
        Ok(reply) => reply,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xreadgroup failed for runner");
            return Ok(Vec::new());
        }
    };
    match parse_read_reply(&reply)? {
        Some(streams) => Ok(decode_read_result(Some(&streams))?),
        None => Ok(Vec::new()),
    }
}

/// Multi-id `XACK`; returns the count of removed PEL entries
/// (`outbox.py:526-538`). Empty (or all-falsy) ids return `0` with
/// no Redis call.
pub async fn ack_for_session(
    client: Option<&redis::Client>,
    runner_id: &str,
    stream_ids: &[String],
) -> Result<usize, OutboxError> {
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
    xack.arg(keys::stream_key(runner_id))
        .arg(keys::group_name(runner_id));
    for id in &ids {
        xack.arg(id);
    }
    let acked: i64 = xack.query_async(&mut connection).await?;
    Ok(usize::try_from(acked).unwrap_or(0))
}

/// Set the per-session PEL-drained marker with a TTL of
/// 2x `access_token_ttl_secs` (`outbox.py:428-434`).
pub async fn mark_pel_drained(
    client: Option<&redis::Client>,
    runner: &RunnerSettings,
    session_id: &str,
) -> Result<(), OutboxError> {
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
/// (`outbox.py:437-441`).
pub async fn is_pel_drained(
    client: Option<&redis::Client>,
    session_id: &str,
) -> Result<bool, OutboxError> {
    let Some(client) = client else {
        return Ok(false);
    };
    let mut connection = connect(client).await?;
    let mut exists = redis::cmd("EXISTS");
    exists.arg(keys::session_pel_drained_key(session_id));
    let found: i64 = exists.query_async(&mut connection).await?;
    Ok(found != 0)
}

/// Delete the per-session PEL-drained marker (`outbox.py:444-449`).
pub async fn clear_session_marker(
    client: Option<&redis::Client>,
    session_id: &str,
) -> Result<(), OutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut delete = redis::cmd("DEL");
    delete.arg(keys::session_pel_drained_key(session_id));
    let _: () = delete.query_async(&mut connection).await?;
    Ok(())
}

/// Publish a session-eviction notice (`outbox.py:544-556`). The
/// subscriber count is ignored, like Python's bare `publish`.
pub async fn publish_session_eviction(
    client: Option<&redis::Client>,
    runner_id: &str,
    old_session_id: Option<&str>,
    new_session_id: &str,
) -> Result<(), OutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut publish = redis::cmd("PUBLISH");
    publish
        .arg(keys::session_eviction_channel(runner_id))
        .arg(eviction_body(old_session_id, new_session_id));
    let _: () = publish.query_async(&mut connection).await?;
    Ok(())
}

/// Mark a runner stream for delayed cleanup (`outbox.py:562-578`).
/// The score is `now + 2 * access_token_ttl_secs` so the daemon
/// has time to observe shutdown.
pub async fn schedule_stream_cleanup_for_runner(
    client: Option<&redis::Client>,
    runner: &RunnerSettings,
    runner_id: &str,
) -> Result<(), OutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let when = epoch_secs() + runner.access_token_ttl_secs * 2;
    let mut zadd = redis::cmd("ZADD");
    zadd.arg(keys::stream_cleanup_zset_key())
        .arg(when)
        .arg(runner_id);
    let _: () = zadd.query_async(&mut connection).await?;
    Ok(())
}

/// Runner ids whose stream-cleanup time has come
/// (`outbox.py:581-591`).
pub async fn due_runners_for_stream_cleanup(
    client: Option<&redis::Client>,
) -> Result<Vec<String>, OutboxError> {
    let Some(client) = client else {
        return Ok(Vec::new());
    };
    let mut connection = connect(client).await?;
    let mut zrangebyscore = redis::cmd("ZRANGEBYSCORE");
    zrangebyscore
        .arg(keys::stream_cleanup_zset_key())
        .arg(0)
        .arg(epoch_secs());
    let reply: redis::Value = zrangebyscore.query_async(&mut connection).await?;
    let members = match &reply {
        redis::Value::Nil => &[][..],
        redis::Value::Array(members) => &members[..],
        _ => {
            return Err(OutboxError::UnexpectedReply(
                "ZRANGEBYSCORE reply not an array",
            ));
        }
    };
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        // `m.decode() if isinstance(m, bytes) else str(m)`.
        match member {
            redis::Value::BulkString(bytes) => {
                out.push(std::str::from_utf8(bytes)?.to_string());
            }
            redis::Value::SimpleString(text) => out.push(text.clone()),
            redis::Value::Int(score) => out.push(score.to_string()),
            _ => {
                return Err(OutboxError::UnexpectedReply(
                    "ZRANGEBYSCORE member not bytes",
                ));
            }
        }
    }
    Ok(out)
}

/// Drop a runner's stream-cleanup marker (`outbox.py:594-598`).
pub async fn remove_stream_cleanup_marker(
    client: Option<&redis::Client>,
    runner_id: &str,
) -> Result<(), OutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut zrem = redis::cmd("ZREM");
    zrem.arg(keys::stream_cleanup_zset_key()).arg(runner_id);
    let _: () = zrem.query_async(&mut connection).await?;
    Ok(())
}

/// Delete a runner's live and offline streams (`outbox.py:601-606`).
pub async fn delete_runner_stream(
    client: Option<&redis::Client>,
    runner_id: &str,
) -> Result<(), OutboxError> {
    let Some(client) = client else {
        return Ok(());
    };
    let mut connection = connect(client).await?;
    let mut delete_live = redis::cmd("DEL");
    delete_live.arg(keys::stream_key(runner_id));
    let _: () = delete_live.query_async(&mut connection).await?;
    let mut delete_offline = redis::cmd("DEL");
    delete_offline.arg(keys::offline_stream_key(runner_id));
    let _: () = delete_offline.query_async(&mut connection).await?;
    Ok(())
}

/// PEL+undelivered-aware `XTRIM MINID` (`outbox.py:609-681`).
///
/// Trims to `min(time_cutoff_id, safe_floor)` where the floor is
/// `min_pending - 1` (or `last_delivered_id` with an empty PEL).
/// Returns the removed count, or `None` when the trim is skipped
/// (no client, group missing, non-monotonic cutoff, or a
/// transport failure — all logged like Python's
/// `logger.exception` sites).
pub async fn safe_trim_runner_stream(
    client: Option<&redis::Client>,
    runner_id: &str,
    time_cutoff_id: &str,
) -> Result<Option<usize>, OutboxError> {
    let Some(client) = client else {
        return Ok(None);
    };
    let sk = keys::stream_key(runner_id);
    let gn = keys::group_name(runner_id);
    let mut connection = match connect(client).await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xinfo groups failed");
            return Ok(None);
        }
    };
    let mut xinfo = redis::cmd("XINFO");
    xinfo.arg("GROUPS").arg(&sk);
    let groups: redis::Value = match xinfo.query_async(&mut connection).await {
        Ok(groups) => groups,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xinfo groups failed");
            return Ok(None);
        }
    };
    let entries = match &groups {
        redis::Value::Nil => &[][..],
        redis::Value::Array(entries) => &entries[..],
        // `for entry in groups` raises on a non-iterable (outside
        // the `try`).
        _ => {
            return Err(OutboxError::UnexpectedReply(
                "XINFO GROUPS reply not an array",
            ));
        }
    };
    let mut last_delivered: Option<String> = None;
    for entry in entries {
        // redis-py always yields dicts here; anything else fails
        // the `entry.get` / `entry[1]` access in Python.
        if as_array(entry).is_none() {
            return Err(OutboxError::UnexpectedReply(
                "XINFO GROUPS entry not shaped",
            ));
        }
        // `str(name) == gn` with `None` reading as `"None"` (never
        // the group): missing or non-bytes names skip the entry.
        let name = match info_field(entry, "name").and_then(as_bytes) {
            Some(name) => std::str::from_utf8(name)?,
            None => continue,
        };
        if name != gn {
            continue;
        }
        let delivered = info_field(entry, "last-delivered-id");
        last_delivered = Some(match delivered {
            // `str(None)`: a missing id poisons the later `min()`
            // and skips the trim, exactly like Python.
            None => "None".to_string(),
            Some(value) => match value {
                redis::Value::BulkString(raw) => std::str::from_utf8(raw)?.to_string(),
                redis::Value::SimpleString(text) => text.clone(),
                redis::Value::Int(count) => count.to_string(),
                // `str(exotic)` never parses as a stream id, so any
                // non-parsing stand-in reaches the same skipped trim.
                _ => "None".to_string(),
            },
        });
        break;
    }
    let Some(last_delivered) = last_delivered else {
        return Ok(None);
    };

    let mut xpending = redis::cmd("XPENDING");
    xpending.arg(&sk).arg(&gn);
    let pending: redis::Value = match xpending.query_async(&mut connection).await {
        Ok(pending) => pending,
        Err(error) => {
            tracing::warn!(%error, runner_id, "xpending failed");
            return Ok(None);
        }
    };
    // The wire shape is the summary array, which is Python's
    // `list`/`tuple` branch (`min` at index 1); anything else
    // leaves `min_pending` unset and the floor falls back to
    // `last_delivered`, exactly like Python's failed `isinstance`
    // checks.
    let min_pending: Option<String> = match &pending {
        redis::Value::Array(parts) if parts.len() >= 2 => match &parts[1] {
            redis::Value::Nil => None,
            redis::Value::BulkString(raw) if raw.is_empty() => None,
            redis::Value::BulkString(raw) => Some(std::str::from_utf8(raw)?.to_string()),
            redis::Value::SimpleString(text) if text.is_empty() => None,
            redis::Value::SimpleString(text) => Some(text.clone()),
            redis::Value::Int(count) => Some(count.to_string()),
            // `str(exotic)` never parses as an id, so the `min()`
            // below yields `None`: return the skipped trim now.
            _ => return Ok(None),
        },
        _ => None,
    };

    let safe_floor = match min_pending {
        Some(ref min) => decrement_stream_id(min),
        None => last_delivered,
    };
    let Some(safe_cutoff) = min_stream_id(Some(time_cutoff_id), Some(&safe_floor)) else {
        return Ok(None);
    };
    let mut xtrim = redis::cmd("XTRIM");
    xtrim.arg(&sk).arg("MINID").arg("~").arg(&safe_cutoff);
    // `int()` sits inside the `try`: an unparsable count logs and
    // skips like a transport failure.
    let trimmed: Result<i64, redis::RedisError> = xtrim.query_async(&mut connection).await;
    match trimmed {
        Ok(count) => Ok(Some(usize::try_from(count).unwrap_or(0))),
        Err(error) => {
            tracing::warn!(%error, runner_id, "xtrim failed");
            Ok(None)
        }
    }
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
            sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/outbox_hermetic")
                .expect("lazy pool parses")
        })
    }

    fn fresh_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    // -- hermetic: validation, messages, None-paths, parsers --------

    #[test]
    fn unknown_type_rejected_before_any_io() {
        // Validation precedes even the client check: `None` plus an
        // unknown type still raises, and the dead pool proves no SQL.
        let pool = dead_pool();
        let runner = runner_settings();
        let rid = uuid::Uuid::new_v4();
        let fx = fixture();
        for (message, recorded) in [
            (message("nope"), "runner"),
            (Map::new(), "runner_missing_type"),
        ] {
            let error = block_on(enqueue_for_runner(None, &pool, &runner, rid, &message))
                .expect_err("unknown type raises");
            let expected = fx["enqueue_unknown_type"][recorded]
                .as_str()
                .expect("fixture vector");
            assert_eq!(
                format!("ValueError: {error}"),
                expected,
                "message matches {recorded}"
            );
        }
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

    #[test]
    fn offline_error_message_matches_python() {
        let fx = fixture();
        let rid = "2b5e80b9-a171-4c81-b154-bdabe36dfa35";
        let error = OutboxError::RunnerOffline {
            runner_id: rid.to_string(),
            message_type: "assign".to_string(),
        };
        let expected = fx["enqueue_offline_reject"]["assign"]
            .as_str()
            .expect("fixture vector");
        assert_eq!(format!("RunnerOfflineError: {error}"), expected);
    }

    #[test]
    fn redis_none_returns_defaults_without_io() {
        let pool = dead_pool();
        let runner = runner_settings();
        let rid = uuid::Uuid::new_v4();
        let rid_str = rid.to_string();
        let sid = fresh_id();
        let fx = fixture();
        let none = &fx["redis_none"];

        let enqueue = block_on(enqueue_for_runner(
            None,
            &pool,
            &runner,
            rid,
            &message("config_push"),
        ));
        assert_eq!(enqueue.expect("enqueue None"), None);
        assert!(none["enqueue"].is_null());

        assert_eq!(
            block_on(drain_offline_into_live(None, &rid_str)).expect("drain"),
            0
        );
        assert_eq!(none["drain"], json!(0));
        assert_eq!(
            block_on(claim_pending_for_new_session(
                None,
                &rid_str,
                Some("consumer-old"),
                "consumer-new",
                0
            )),
            0
        );
        assert_eq!(none["claim"], json!(0));
        assert_eq!(
            block_on(delete_consumer(None, &rid_str, Some("consumer-x"))),
            0
        );
        assert_eq!(none["delete_consumer"], json!(0));
        assert_eq!(
            block_on(reap_idle_consumers(
                None,
                &runner,
                &rid_str,
                &HashSet::new(),
                None
            ))
            .expect("reap"),
            0
        );
        assert_eq!(none["reap_consumers"], json!(0));
        block_on(mark_pel_drained(None, &runner, &sid)).expect("mark");
        assert!(
            !block_on(is_pel_drained(None, &sid)).expect("is drained"),
            "None reads as not drained"
        );
        assert_eq!(none["is_pel_drained"], json!(false));
        block_on(clear_session_marker(None, &sid)).expect("clear");
        assert!(
            block_on(read_for_session(None, &rid_str, &sid, 0, 100, false))
                .expect("read")
                .is_empty()
        );
        assert_eq!(none["read"], json!([]));
        assert_eq!(
            block_on(ack_for_session(None, &rid_str, &["1-0".to_string()])).expect("ack"),
            0
        );
        assert_eq!(none["ack"], json!(0));
        block_on(publish_session_eviction(None, &rid_str, None, &sid)).expect("publish");
        block_on(schedule_stream_cleanup_for_runner(None, &runner, &rid_str)).expect("schedule");
        assert!(block_on(due_runners_for_stream_cleanup(None))
            .expect("due")
            .is_empty());
        assert_eq!(none["due_cleanup"], json!([]));
        block_on(remove_stream_cleanup_marker(None, &rid_str)).expect("remove");
        block_on(delete_runner_stream(None, &rid_str)).expect("delete");
        block_on(ensure_stream_group(None, &rid_str)).expect("ensure");
        assert!(none["ensure_group"].is_null());
        assert_eq!(
            block_on(safe_trim_runner_stream(None, &rid_str, "9-0")).expect("trim"),
            None
        );
        assert!(none["safe_trim"].is_null());
    }

    #[test]
    fn falsy_consumer_and_empty_ack_skip_redis_entirely() {
        // A dead-port client proves no connection is even attempted:
        // the falsy checks precede everything.
        let client = redis::Client::open("redis://127.0.0.1:1/").expect("dead-port client parses");
        let rid = fresh_id();
        assert_eq!(block_on(delete_consumer(Some(&client), &rid, None)), 0);
        assert_eq!(block_on(delete_consumer(Some(&client), &rid, Some(""))), 0);
        assert_eq!(
            block_on(ack_for_session(Some(&client), &rid, &[])).expect("ack"),
            0
        );
        assert_eq!(
            block_on(ack_for_session(
                Some(&client),
                &rid,
                &["".to_string(), String::new()]
            ))
            .expect("ack falsy"),
            0
        );
        let fx = fixture();
        assert_eq!(fx["delete_consumer_falsy"]["none"], json!(0));
        assert_eq!(fx["delete_consumer_falsy"]["empty"], json!(0));
        assert_eq!(fx["ack_for_session"]["empty"], json!(0));
        assert_eq!(fx["ack_for_session"]["falsy"], json!(0));
    }

    #[test]
    fn consumer_stat_vectors() {
        use redis::Value::{Array, BulkString, Int, Nil};
        let make_entry = |pairs: Vec<redis::Value>| Array(pairs);
        let bytes = |s: &str| BulkString(s.as_bytes().to_vec());

        // Missing reads as 0 (the `_consumer_info_value` default).
        assert_eq!(
            consumer_stat(&Array(vec![]), "pending").expect("missing"),
            0
        );
        assert_eq!(consumer_stat(&Nil, "idle").expect("nil entry"), 0);
        // Integers pass through, including 0 and negatives.
        let entry = make_entry(vec![bytes("pending"), Int(3)]);
        assert_eq!(consumer_stat(&entry, "pending").expect("int"), 3);
        let entry = make_entry(vec![bytes("idle"), Int(0)]);
        assert_eq!(consumer_stat(&entry, "idle").expect("zero"), 0);
        // Numeric bytes parse; empty bytes are falsy (`or 0`).
        let entry = make_entry(vec![bytes("pending"), bytes("42")]);
        assert_eq!(consumer_stat(&entry, "pending").expect("bytes"), 42);
        let entry = make_entry(vec![bytes("idle"), bytes("")]);
        assert_eq!(consumer_stat(&entry, "idle").expect("empty"), 0);
        let entry = make_entry(vec![bytes("idle"), bytes("  7  ")]);
        assert_eq!(consumer_stat(&entry, "idle").expect("padded"), 7);
        // Anything `int()` rejects raises in Python (`Err` here).
        let entry = make_entry(vec![bytes("pending"), bytes("lots")]);
        assert!(consumer_stat(&entry, "pending").is_err());
        let entry = make_entry(vec![bytes("pending"), BulkString(vec![0xff])]);
        assert!(consumer_stat(&entry, "pending").is_err());
        let entry = make_entry(vec![bytes("pending"), Array(vec![Int(1)])]);
        assert!(consumer_stat(&entry, "pending").is_err());
    }

    #[test]
    fn consumer_name_vectors() {
        use redis::Value::{Array, BulkString, Int, Nil};
        let bytes = |s: &str| BulkString(s.as_bytes().to_vec());

        assert_eq!(consumer_name(&Array(vec![])).expect("missing"), None);
        assert_eq!(consumer_name(&Nil).expect("nil entry"), None);
        let entry = Array(vec![bytes("name"), bytes("consumer-a")]);
        assert_eq!(
            consumer_name(&entry).expect("bytes"),
            Some("consumer-a".to_string())
        );
        // Empty decodes (the caller skips it, like `if not name`).
        let entry = Array(vec![bytes("name"), bytes("")]);
        assert_eq!(consumer_name(&entry).expect("empty"), Some(String::new()));
        let entry = Array(vec![bytes("name"), Int(7)]);
        assert_eq!(consumer_name(&entry).expect("int"), Some("7".to_string()));
        // Integer zero is falsy in Python (`if not name` skips it).
        let entry = Array(vec![bytes("name"), Int(0)]);
        assert_eq!(consumer_name(&entry).expect("zero"), None);
        let entry = Array(vec![bytes("name"), BulkString(vec![0xff, 0xfe])]);
        assert!(matches!(
            consumer_name(&entry),
            Err(OutboxError::InvalidUtf8(_))
        ));
    }

    #[test]
    fn autoclaim_parse_vectors() {
        use redis::Value::{Array, BulkString, Int};
        let bytes = |s: &str| BulkString(s.as_bytes().to_vec());

        // Empty PEL: cursor plus no ids.
        let reply = Array(vec![bytes("0-0"), Array(vec![])]);
        assert_eq!(parse_autoclaim(&reply), Some(("0-0".to_string(), vec![])));
        // A page: cursor plus ids; a Redis 7 third element is ignored.
        let reply = Array(vec![
            bytes("100-5"),
            Array(vec![bytes("90-0"), bytes("91-0")]),
            Array(vec![]),
        ]);
        let (cursor, ids) = parse_autoclaim(&reply).expect("page parses");
        assert_eq!(cursor, "100-5");
        assert_eq!(ids, vec![b"90-0".to_vec(), b"91-0".to_vec()]);
        // Shapes a real server never sends read as failures.
        assert_eq!(parse_autoclaim(&Array(vec![])), None);
        assert_eq!(parse_autoclaim(&Int(3)), None);
        assert_eq!(parse_autoclaim(&Array(vec![Int(3), Array(vec![])])), None);
        assert_eq!(
            parse_autoclaim(&Array(vec![bytes("0-0"), bytes("90-0")])),
            None
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
            bytes(b"runner_stream:r"),
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
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let expected = argv([
            "XGROUP",
            "CREATE",
            sk.as_str(),
            gn.as_str(),
            "$",
            "MKSTREAM",
        ]);

        let (client, log) = start_mock(vec![resp_ok()]);
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        assert_eq!(recorded(&log), vec![expected.clone()]);
        drop(client);

        // BUSYGROUP means the group exists: swallowed.
        let (client, log) = start_mock(vec![resp_err(
            "BUSYGROUP Consumer Group name already exists",
        )]);
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("busygroup swallowed");
        assert_eq!(recorded(&log), vec![expected]);
        drop(client);

        // Anything else propagates.
        let (client, _) = start_mock(vec![resp_err("NOGROUP missing")]);
        assert!(matches!(
            ensure_stream_group(Some(&client), &rid).await,
            Err(OutboxError::Redis(_))
        ));
    }

    #[tokio::test]
    async fn mock_drain_offline_sequence() {
        let rid = fresh_id();
        let okey = keys::offline_stream_key(&rid);
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);

        // Empty buffer: XRANGE only, returns 0 (drain_offline_empty).
        let (client, log) = start_mock(vec![resp_array(vec![])]);
        assert_eq!(
            drain_offline_into_live(Some(&client), &rid)
                .await
                .expect("drain"),
            0
        );
        assert_eq!(
            recorded(&log),
            vec![argv(["XRANGE", okey.as_str(), "-", "+"])]
        );
        drop(client);

        // One entry: XRANGE, XGROUP, XADD (field order kept), DEL.
        let xrange = resp_array(vec![resp_array(vec![
            resp_str("1790987270367-0"),
            resp_array(vec![
                resp_str("mid"),
                resp_str("m1"),
                resp_str("type"),
                resp_str("config_push"),
                resp_str("payload"),
                resp_str("{\"mid\": \"m1\"}"),
            ]),
        ])]);
        let (client, log) = start_mock(vec![
            xrange,
            resp_ok(),
            resp_str("1790987270370-0"),
            resp_int(1),
        ]);
        assert_eq!(
            drain_offline_into_live(Some(&client), &rid)
                .await
                .expect("drain"),
            1
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
                    "m1",
                    "type",
                    "config_push",
                    "payload",
                    "{\"mid\": \"m1\"}"
                ]),
                argv(["DEL", okey.as_str()]),
            ]
        );
    }

    #[tokio::test]
    async fn mock_drain_bad_utf8_raises() {
        // Non-UTF-8 field bytes: Python's `.decode()` raises, so the
        // drain fails before the group call.
        let rid = fresh_id();
        let xrange = resp_array(vec![resp_array(vec![
            resp_str("9-0"),
            resp_array(vec![resp_bytes(b"mid"), resp_bytes(&[0xff, 0xfe])]),
        ])]);
        let (client, log) = start_mock(vec![xrange, resp_ok()]);
        let result = drain_offline_into_live(Some(&client), &rid).await;
        assert!(matches!(result, Err(OutboxError::InvalidUtf8(_))));
        assert_eq!(recorded(&log).len(), 2, "fails after XRANGE + XGROUP");
    }

    #[tokio::test]
    async fn mock_claim_empty_pel_returns_without_delete() {
        // Empty PEL: one XAUTOCLAIM, returns 0, and NO DELCONSUMER —
        // the old consumer is kept (claim_pending_empty_pel).
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let empty = resp_array(vec![resp_str("0-0"), resp_array(vec![])]);
        let (client, log) = start_mock(vec![empty]);
        let claimed = claim_pending_for_new_session(
            Some(&client),
            &rid,
            Some("consumer-old"),
            "consumer-new",
            0,
        )
        .await;
        assert_eq!(claimed, 0);
        assert_eq!(
            recorded(&log),
            vec![argv([
                "XAUTOCLAIM",
                sk.as_str(),
                gn.as_str(),
                "consumer-new",
                "0",
                "0-0",
                "COUNT",
                "200",
                "JUSTID"
            ])]
        );
    }

    #[tokio::test]
    async fn mock_claim_paginates_then_deletes_old_consumer() {
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let page1 = resp_array(vec![
            resp_str("100-5"),
            resp_array(vec![resp_str("90-0"), resp_str("91-0")]),
        ]);
        let page2 = resp_array(vec![resp_str("0-0"), resp_array(vec![resp_str("100-5")])]);
        let (client, log) = start_mock(vec![page1, page2, resp_int(0)]);
        let claimed = claim_pending_for_new_session(
            Some(&client),
            &rid,
            Some("consumer-old"),
            "consumer-new",
            0,
        )
        .await;
        assert_eq!(claimed, 3);
        assert_eq!(
            recorded(&log),
            vec![
                argv([
                    "XAUTOCLAIM",
                    sk.as_str(),
                    gn.as_str(),
                    "consumer-new",
                    "0",
                    "0-0",
                    "COUNT",
                    "200",
                    "JUSTID"
                ]),
                argv([
                    "XAUTOCLAIM",
                    sk.as_str(),
                    gn.as_str(),
                    "consumer-new",
                    "0",
                    "100-5",
                    "COUNT",
                    "200",
                    "JUSTID"
                ]),
                argv([
                    "XGROUP",
                    "DELCONSUMER",
                    sk.as_str(),
                    gn.as_str(),
                    "consumer-old"
                ]),
            ]
        );
    }

    #[tokio::test]
    async fn mock_claim_failures_return_count_so_far() {
        let rid = fresh_id();
        // Error on page 2: the page-1 id still counts.
        let page1 = resp_array(vec![resp_str("90-0"), resp_array(vec![resp_str("90-0")])]);
        let (client, log) = start_mock(vec![page1, resp_err("NOGROUP gone")]);
        let claimed =
            claim_pending_for_new_session(Some(&client), &rid, None, "consumer-new", 0).await;
        assert_eq!(claimed, 1);
        assert_eq!(recorded(&log).len(), 2);
        drop(client);

        // An unshaped reply reads as a failure, not a hang.
        let (client, log) = start_mock(vec![resp_int(5)]);
        let claimed =
            claim_pending_for_new_session(Some(&client), &rid, None, "consumer-new", 0).await;
        assert_eq!(claimed, 0);
        assert_eq!(recorded(&log).len(), 1);
    }

    #[tokio::test]
    async fn mock_delete_consumer_argv_and_swallow() {
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let expected = argv([
            "XGROUP",
            "DELCONSUMER",
            sk.as_str(),
            gn.as_str(),
            "consumer-old",
        ]);

        let (client, log) = start_mock(vec![resp_int(1)]);
        assert_eq!(
            delete_consumer(Some(&client), &rid, Some("consumer-old")).await,
            1
        );
        assert_eq!(recorded(&log), vec![expected]);
        drop(client);

        // Transport failures log and return 0.
        let (client, log) = start_mock(vec![resp_err("NOGROUP gone")]);
        assert_eq!(
            delete_consumer(Some(&client), &rid, Some("consumer-old")).await,
            0
        );
        assert_eq!(recorded(&log).len(), 1);
    }

    #[tokio::test]
    async fn mock_read_decodes_fixture_vectors() {
        // The fixture's `read_for_session.gt` entries, served raw:
        // rd1 carries an extra field, rd3 a corrupt payload.
        let rid = fresh_id();
        let sid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let cn = keys::consumer_name(&sid);
        let entry = |id: &str, mid: &str, payload: &str| {
            resp_array(vec![
                resp_str(id),
                resp_array(vec![
                    resp_str("mid"),
                    resp_str(mid),
                    resp_str("type"),
                    resp_str("config_push"),
                    resp_str("payload"),
                    resp_str(payload),
                ]),
            ])
        };
        let reply = resp_array(vec![resp_array(vec![
            resp_str(&sk),
            resp_array(vec![
                entry(
                    "1790987273758-0",
                    "rd1",
                    "{\"type\": \"config_push\", \"mid\": \"rd1\", \"n\": 1}",
                ),
                entry(
                    "1790987273760-0",
                    "rd2",
                    "{\"type\": \"config_push\", \"mid\": \"rd2\"}",
                ),
                entry("1790987273760-1", "rd3", "{corrupt"),
            ]),
        ])]);
        let (client, log) = start_mock(vec![resp_ok(), reply]);
        let messages = read_for_session(Some(&client), &rid, &sid, 0, 100, false)
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
        let expected = fx["read_for_session"]["gt"].as_array().expect("gt");
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
        let messages = read_for_session(Some(&client), &rid, &sid, 0, 100, true)
            .await
            .expect("read zero");
        assert!(messages.is_empty());
        assert_eq!(recorded(&log)[1].last().map(String::as_str), Some("0"));
        drop(client);

        // Transport failures log and return no entries; nil (block
        // timeout) reads as absent.
        let (client, _) = start_mock(vec![resp_ok(), resp_err("boom")]);
        assert!(read_for_session(Some(&client), &rid, &sid, 0, 100, false)
            .await
            .expect("read error")
            .is_empty());
        drop(client);
        let (client, _) = start_mock(vec![resp_ok(), b"$-1\r\n".to_vec()]);
        assert!(read_for_session(Some(&client), &rid, &sid, 0, 100, false)
            .await
            .expect("read nil")
            .is_empty());
    }

    #[tokio::test]
    async fn mock_ack_argv() {
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let ids = ["1790987273758-0", "1790987273760-0", "1790987273760-1"];
        let (client, log) = start_mock(vec![resp_int(3)]);
        let owned: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(
            ack_for_session(Some(&client), &rid, &owned)
                .await
                .expect("ack"),
            3
        );
        assert_eq!(
            recorded(&log),
            vec![argv([
                "XACK",
                sk.as_str(),
                gn.as_str(),
                "1790987273758-0",
                "1790987273760-0",
                "1790987273760-1"
            ])]
        );
        let fx = fixture();
        assert_eq!(fx["ack_for_session"]["acked"], json!(3));
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
    }

    #[tokio::test]
    async fn mock_eviction_publish_bodies() {
        let rid = fresh_id();
        let channel = keys::session_eviction_channel(&rid);
        let (client, log) = start_mock(vec![resp_int(0), resp_int(0)]);
        publish_session_eviction(Some(&client), &rid, Some("old-sid"), "new-sid")
            .await
            .expect("publish");
        publish_session_eviction(Some(&client), &rid, None, "new-sid")
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
    async fn mock_cleanup_zset_sequence() {
        let runner = runner_settings();
        let rid = fresh_id();
        let zset = keys::stream_cleanup_zset_key().to_string();
        let (client, log) = start_mock(vec![
            resp_int(1),
            resp_array(vec![resp_str(&rid)]),
            resp_int(1),
        ]);
        let before = epoch_secs();
        schedule_stream_cleanup_for_runner(Some(&client), &runner, &rid)
            .await
            .expect("schedule");
        let due = due_runners_for_stream_cleanup(Some(&client))
            .await
            .expect("due");
        remove_stream_cleanup_marker(Some(&client), &rid)
            .await
            .expect("remove");
        let after = epoch_secs();
        assert_eq!(due, vec![rid.clone()]);

        let commands = recorded(&log);
        assert_eq!(commands.len(), 3);
        // ZADD <zset> <now + 7200> <rid>, tolerant of a second tick.
        assert_eq!(commands[0][0], "ZADD");
        assert_eq!(commands[0][1], zset);
        let when: i64 = commands[0][2].parse().expect("score parses");
        assert!(
            (before + 7200..=after + 7200).contains(&when),
            "score {when} is now + 2x ttl"
        );
        assert_eq!(commands[0][3], rid);
        // ZRANGEBYSCORE <zset> 0 <now>.
        assert_eq!(commands[1][0], "ZRANGEBYSCORE");
        assert_eq!(commands[1][1], zset);
        assert_eq!(commands[1][2], "0");
        let now: i64 = commands[1][3].parse().expect("now parses");
        assert!((before..=after).contains(&now));
        assert_eq!(commands[2], argv(["ZREM", zset.as_str(), rid.as_str()]));
    }

    #[tokio::test]
    async fn mock_delete_stream_order() {
        // Live stream first, offline buffer second.
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let okey = keys::offline_stream_key(&rid);
        let (client, log) = start_mock(vec![resp_int(1), resp_int(1)]);
        delete_runner_stream(Some(&client), &rid)
            .await
            .expect("delete");
        assert_eq!(
            recorded(&log),
            vec![argv(["DEL", sk.as_str()]), argv(["DEL", okey.as_str()])]
        );
    }

    #[tokio::test]
    async fn mock_reap_selects_and_sums_pending() {
        // Four consumers: only the stale zero-pending one goes, and
        // the return sums pending counts (0), not removals.
        let runner = runner_settings();
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let consumer = |name: &str, pending: i64, idle: i64| {
            resp_array(vec![
                resp_str("name"),
                resp_str(name),
                resp_str("pending"),
                resp_int(pending),
                resp_str("idle"),
                resp_int(idle),
            ])
        };
        let xinfo = resp_array(vec![
            consumer("consumer-idle", 0, 999_999),
            consumer("consumer-keep", 0, 999_999),
            consumer("consumer-pend", 1, 999_999),
            consumer("consumer-fresh", 0, 5),
        ]);
        let (client, log) = start_mock(vec![xinfo, resp_int(0)]);
        let mut keep = HashSet::new();
        keep.insert("consumer-keep".to_string());
        let reaped = reap_idle_consumers(Some(&client), &runner, &rid, &keep, Some(1000))
            .await
            .expect("reap");
        assert_eq!(reaped, 0, "sums pending counts, not removals");
        assert_eq!(
            recorded(&log),
            vec![
                argv(["XINFO", "CONSUMERS", sk.as_str(), gn.as_str()]),
                argv([
                    "XGROUP",
                    "DELCONSUMER",
                    sk.as_str(),
                    gn.as_str(),
                    "consumer-idle"
                ]),
            ]
        );
        drop(client);

        // The default floor is max(25 * 4 * 1000, 120_000).
        let xinfo = resp_array(vec![
            consumer("consumer-df", 0, 130_000),
            consumer("consumer-young", 0, 100_000),
        ]);
        let (client, log) = start_mock(vec![xinfo, resp_int(0)]);
        let reaped = reap_idle_consumers(Some(&client), &runner, &rid, &HashSet::new(), None)
            .await
            .expect("reap default floor");
        assert_eq!(reaped, 0);
        assert_eq!(recorded(&log).len(), 2);
        assert_eq!(recorded(&log)[1][4], "consumer-df");
        drop(client);

        // XINFO failures log and return 0.
        let (client, log) = start_mock(vec![resp_err("NOGROUP gone")]);
        assert_eq!(
            reap_idle_consumers(Some(&client), &runner, &rid, &HashSet::new(), None)
                .await
                .expect("reap error"),
            0
        );
        assert_eq!(recorded(&log).len(), 1);
    }

    #[tokio::test]
    async fn mock_trim_sequences() {
        let rid = fresh_id();
        let sk = keys::stream_key(&rid);
        let gn = keys::group_name(&rid);
        let groups = |delivered: &str| {
            resp_array(vec![resp_array(vec![
                resp_str("name"),
                resp_str(&gn),
                resp_str("consumers"),
                resp_int(1),
                resp_str("pending"),
                resp_int(1),
                resp_str("last-delivered-id"),
                resp_str(delivered),
            ])])
        };
        let pending_summary = |min: &str| {
            resp_array(vec![
                resp_int(1),
                resp_str(min),
                resp_str(min),
                resp_array(vec![]),
            ])
        };

        // Mirror of fixture `safe_trim`: cutoff far in the future,
        // floor min_pending - 1, trim sent as MINID ~.
        let (client, log) = start_mock(vec![
            groups("1790987274107-1"),
            pending_summary("1790987274107-1"),
            resp_int(0),
        ]);
        let trimmed = safe_trim_runner_stream(Some(&client), &rid, "1790990874103-0")
            .await
            .expect("trim");
        assert_eq!(trimmed, Some(0));
        assert_eq!(
            recorded(&log),
            vec![
                argv(["XINFO", "GROUPS", sk.as_str()]),
                argv(["XPENDING", sk.as_str(), gn.as_str()]),
                argv(["XTRIM", sk.as_str(), "MINID", "~", "1790987274107-0"]),
            ]
        );
        drop(client);

        // No group: None after one call (safe_trim_no_group).
        let (client, log) = start_mock(vec![resp_array(vec![])]);
        assert_eq!(
            safe_trim_runner_stream(Some(&client), &rid, "9-0")
                .await
                .expect("trim"),
            None
        );
        assert_eq!(recorded(&log).len(), 1);
        drop(client);

        // Empty PEL: the floor is last-delivered (500-0), so a
        // 999-0 cutoff trims to MINID ~ 500-0.
        let empty_pending = resp_array(vec![
            resp_int(0),
            b"$-1\r\n".to_vec(),
            b"$-1\r\n".to_vec(),
            resp_array(vec![]),
        ]);
        let (client, log) = start_mock(vec![groups("500-0"), empty_pending, resp_int(2)]);
        assert_eq!(
            safe_trim_runner_stream(Some(&client), &rid, "999-0")
                .await
                .expect("trim"),
            Some(2)
        );
        assert_eq!(
            recorded(&log)[2],
            argv(["XTRIM", sk.as_str(), "MINID", "~", "500-0"])
        );
        drop(client);

        // A non-monotonic cutoff skips the trim (no XTRIM call).
        let (client, log) = start_mock(vec![
            groups("1790987274107-1"),
            pending_summary("1790987274107-1"),
        ]);
        assert_eq!(
            safe_trim_runner_stream(Some(&client), &rid, "garbage")
                .await
                .expect("trim"),
            None
        );
        assert_eq!(recorded(&log).len(), 2);
        drop(client);

        // XPENDING / XTRIM failures log and skip.
        let (client, log) = start_mock(vec![groups("500-0"), resp_err("boom")]);
        assert_eq!(
            safe_trim_runner_stream(Some(&client), &rid, "999-0")
                .await
                .expect("trim"),
            None
        );
        assert_eq!(recorded(&log).len(), 2);
        drop(client);
        let (client, log) = start_mock(vec![
            groups("500-0"),
            pending_summary("500-0"),
            resp_err("boom"),
        ]);
        assert_eq!(
            safe_trim_runner_stream(Some(&client), &rid, "999-0")
                .await
                .expect("trim"),
            None
        );
        assert_eq!(recorded(&log).len(), 3);
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

    /// Delete a test's keys plus its cleanup-zset member
    /// (best-effort order; every live test calls this).
    async fn cleanup(client: &redis::Client, runner_id: &str, session_ids: &[&str]) {
        let mut conn = live_conn(client).await;
        let mut delete = redis::cmd("DEL");
        delete
            .arg(keys::stream_key(runner_id))
            .arg(keys::offline_stream_key(runner_id));
        for sid in session_ids {
            delete.arg(keys::session_pel_drained_key(sid));
        }
        let _: () = delete.query_async(&mut conn).await.expect("cleanup del");
        let mut zrem = redis::cmd("ZREM");
        zrem.arg(keys::stream_cleanup_zset_key()).arg(runner_id);
        let _: () = zrem.query_async(&mut conn).await.expect("cleanup zrem");
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

    fn xrange_ids(reply: &redis::Value) -> Vec<String> {
        match reply {
            redis::Value::Array(entries) => entries
                .iter()
                .filter_map(|entry| {
                    as_array(entry)
                        .and_then(|pair| pair.first())
                        .and_then(as_bytes)
                        .and_then(|id| std::str::from_utf8(id).ok())
                        .map(str::to_string)
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn consumer_names(reply: &redis::Value) -> Vec<String> {
        match reply {
            redis::Value::Array(entries) => entries
                .iter()
                .filter_map(|entry| {
                    info_field(entry, "name")
                        .and_then(as_bytes)
                        .and_then(|name| std::str::from_utf8(name).ok())
                        .map(str::to_string)
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    #[tokio::test]
    async fn live_drain_moves_buffer_to_stream() {
        let client = live_client();
        let rid = fresh_id();
        let mut conn = live_conn(&client).await;
        raw_xadd(
            &mut conn,
            &keys::offline_stream_key(&rid),
            "*",
            "m-off-1",
            "config_push",
            "{\"mid\": \"m-off-1\"}",
        )
        .await;

        let moved = drain_offline_into_live(Some(&client), &rid)
            .await
            .expect("drain");
        assert_eq!(moved, 1);
        assert_eq!(raw_xlen(&mut conn, &keys::stream_key(&rid)).await, 1);
        assert_eq!(
            raw_exists(&mut conn, &keys::offline_stream_key(&rid)).await,
            0,
            "buffer deleted after the move"
        );
        // The moved entry keeps its fields.
        let mut xrange = redis::cmd("XRANGE");
        xrange.arg(keys::stream_key(&rid)).arg("-").arg("+");
        let reply: redis::Value = xrange.query_async(&mut conn).await.expect("xrange");
        let entries = as_array(&reply).expect("entries array");
        assert_eq!(entries.len(), 1);
        let flat = as_array(&as_array(&entries[0]).expect("pair")[1]).expect("fields");
        let field = |name: &str| {
            flat.chunks(2)
                .find(|pair| pair.len() == 2 && as_bytes(&pair[0]) == Some(name.as_bytes()))
                .and_then(|pair| as_bytes(&pair[1]))
                .and_then(|value| std::str::from_utf8(value).ok())
                .map(str::to_string)
        };
        assert_eq!(field("mid"), Some("m-off-1".to_string()));
        assert_eq!(field("type"), Some("config_push".to_string()));
        // Draining again moves nothing (drain_offline_empty).
        assert_eq!(
            drain_offline_into_live(Some(&client), &rid)
                .await
                .expect("drain again"),
            0
        );
        cleanup(&client, &rid, &[]).await;
    }

    #[tokio::test]
    async fn live_claim_empty_pel_keeps_old_consumer() {
        let client = live_client();
        let rid = fresh_id();
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        let mut create = redis::cmd("XGROUP");
        create
            .arg("CREATECONSUMER")
            .arg(keys::stream_key(&rid))
            .arg(keys::group_name(&rid))
            .arg("consumer-old");
        let _: () = create.query_async(&mut conn).await.expect("mkconsumer");

        let claimed = claim_pending_for_new_session(
            Some(&client),
            &rid,
            Some("consumer-old"),
            "consumer-new",
            0,
        )
        .await;
        assert_eq!(claimed, 0);
        let mut xinfo = redis::cmd("XINFO");
        xinfo
            .arg("CONSUMERS")
            .arg(keys::stream_key(&rid))
            .arg(keys::group_name(&rid));
        let reply: redis::Value = xinfo.query_async(&mut conn).await.expect("xinfo");
        let names = consumer_names(&reply);
        assert!(
            names.contains(&"consumer-old".to_string()),
            "old consumer kept on empty PEL: {names:?}"
        );
        assert!(
            names.contains(&"consumer-new".to_string()),
            "claim registers the new consumer: {names:?}"
        );
        cleanup(&client, &rid, &[]).await;
    }

    #[tokio::test]
    async fn live_claim_nonempty_terminates_and_hands_off() {
        // KNOWN DIVERGENCE (see the module docs): Python spins here
        // forever (the JUSTID cursor bug); the Rust loop paginates
        // to `0-0`, returns the claimed count, and deletes the old
        // consumer.
        let client = live_client();
        let rid = fresh_id();
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        for n in 1..=3 {
            raw_xadd(
                &mut conn,
                &keys::stream_key(&rid),
                "*",
                &format!("m{n}"),
                "config_push",
                "{}",
            )
            .await;
        }
        // Reading as the old consumer parks 3 PEL entries on it.
        let mut read = redis::cmd("XREADGROUP");
        read.arg("GROUP")
            .arg(keys::group_name(&rid))
            .arg("consumer-old")
            .arg("COUNT")
            .arg(10)
            .arg("STREAMS")
            .arg(keys::stream_key(&rid))
            .arg(">");
        let _: redis::Value = read.query_async(&mut conn).await.expect("seed pel");

        let claimed = claim_pending_for_new_session(
            Some(&client),
            &rid,
            Some("consumer-old"),
            "consumer-new",
            0,
        )
        .await;
        assert_eq!(claimed, 3);
        let mut xinfo = redis::cmd("XINFO");
        xinfo
            .arg("CONSUMERS")
            .arg(keys::stream_key(&rid))
            .arg(keys::group_name(&rid));
        let reply: redis::Value = xinfo.query_async(&mut conn).await.expect("xinfo");
        let names = consumer_names(&reply);
        assert!(
            !names.contains(&"consumer-old".to_string()),
            "old consumer deleted after cursor 0-0: {names:?}"
        );
        assert!(names.contains(&"consumer-new".to_string()));
        let mut xpending = redis::cmd("XPENDING");
        xpending
            .arg(keys::stream_key(&rid))
            .arg(keys::group_name(&rid));
        let reply: redis::Value = xpending.query_async(&mut conn).await.expect("xpending");
        let parts = as_array(&reply).expect("pending summary");
        assert_eq!(parts.len(), 4);
        assert!(matches!(parts[0], redis::Value::Int(3)), "3 still pending");
        cleanup(&client, &rid, &[]).await;
    }

    #[tokio::test]
    async fn live_error_paths_on_missing_stream() {
        let client = live_client();
        let rid = fresh_id();
        let runner = runner_settings();
        // NOGROUP / no-such-key failures read as the defaults.
        assert_eq!(
            claim_pending_for_new_session(Some(&client), &rid, None, "consumer-new", 0).await,
            0
        );
        assert_eq!(
            delete_consumer(Some(&client), &rid, Some("consumer-old")).await,
            0
        );
        assert_eq!(
            reap_idle_consumers(Some(&client), &runner, &rid, &HashSet::new(), None)
                .await
                .expect("reap"),
            0
        );
        assert_eq!(
            safe_trim_runner_stream(Some(&client), &rid, "9-0")
                .await
                .expect("trim"),
            None
        );
        // A read creates the group (MKSTREAM) and finds nothing.
        let sid = fresh_id();
        assert!(read_for_session(Some(&client), &rid, &sid, 1, 100, false)
            .await
            .expect("read")
            .is_empty());
        let mut conn = live_conn(&client).await;
        assert_eq!(raw_exists(&mut conn, &keys::stream_key(&rid)).await, 1);
        cleanup(&client, &rid, &[&sid]).await;
    }

    #[tokio::test]
    async fn live_read_ack_roundtrip() {
        let client = live_client();
        let rid = fresh_id();
        let sid = fresh_id();
        // The group must exist before seeding: `>` only sees
        // entries added after the group.
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        raw_xadd(
            &mut conn,
            &keys::stream_key(&rid),
            "*",
            "rd1",
            "config_push",
            "{\"type\": \"config_push\", \"mid\": \"rd1\", \"n\": 1}",
        )
        .await;
        raw_xadd(
            &mut conn,
            &keys::stream_key(&rid),
            "*",
            "rd2",
            "config_push",
            "{\"type\": \"config_push\", \"mid\": \"rd2\"}",
        )
        .await;
        raw_xadd(
            &mut conn,
            &keys::stream_key(&rid),
            "*",
            "rd3",
            "config_push",
            "{corrupt",
        )
        .await;

        let messages = read_for_session(Some(&client), &rid, &sid, 1, 100, false)
            .await
            .expect("read");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].mid, "rd1");
        assert_eq!(messages[0].body["n"], json!(1));
        assert_eq!(messages[2].mid, "rd3");
        assert_eq!(messages[2].body, json!({}));
        let ids: Vec<String> = messages.iter().map(|m| m.stream_id.clone()).collect();
        assert_eq!(
            ack_for_session(Some(&client), &rid, &ids)
                .await
                .expect("ack"),
            3
        );
        assert_eq!(
            ack_for_session(Some(&client), &rid, &ids)
                .await
                .expect("ack again"),
            0
        );
        // PEL replay after the ack finds nothing (use_zero_n 0).
        assert!(read_for_session(Some(&client), &rid, &sid, 1, 100, true)
            .await
            .expect("read zero")
            .is_empty());
        let fx = fixture();
        assert_eq!(fx["ack_for_session"]["acked"], json!(3));
        assert_eq!(fx["ack_for_session"]["acked_again"], json!(0));
        assert_eq!(fx["read_for_session"]["use_zero_n"], json!(0));
        cleanup(&client, &rid, &[&sid]).await;
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
        assert_eq!(fx["pel_marker"]["ttl"], json!(7200));
        assert!(fx["pel_marker"]["ttl_is_2x"].as_bool().expect("2x"));
        cleanup(&client, &fresh_id(), &[&sid]).await;
    }

    #[tokio::test]
    async fn live_eviction_publish_payload() {
        use futures_util::StreamExt;

        let client = live_client();
        let rid = fresh_id();
        let channel = keys::session_eviction_channel(&rid);
        let mut pubsub = client.get_async_pubsub().await.expect("pubsub");
        pubsub.subscribe(&channel).await.expect("subscribe");
        publish_session_eviction(Some(&client), &rid, Some("old-sid"), "new-sid")
            .await
            .expect("publish");
        let message = pubsub.on_message().next().await.expect("published");
        assert_eq!(
            message.get_payload_bytes(),
            b"{\"old_sid\": \"old-sid\", \"new_sid\": \"new-sid\"}"
        );
    }

    #[tokio::test]
    async fn live_cleanup_zset_roundtrip() {
        let client = live_client();
        let runner = runner_settings();
        let rid = fresh_id();
        let before = epoch_secs();
        schedule_stream_cleanup_for_runner(Some(&client), &runner, &rid)
            .await
            .expect("schedule");
        let after = epoch_secs();
        let mut conn = live_conn(&client).await;
        let mut zscore = redis::cmd("ZSCORE");
        zscore.arg(keys::stream_cleanup_zset_key()).arg(&rid);
        let score: String = zscore.query_async(&mut conn).await.expect("zscore");
        let score: i64 = score.parse().expect("score parses");
        assert!(
            (before + 7200..=after + 7200).contains(&score),
            "score is now + 2x ttl: {score}"
        );
        // Not due yet; due after a backdate; gone after removal.
        // (Only our own member is asserted: parallel tests share
        // the zset.)
        let due = due_runners_for_stream_cleanup(Some(&client))
            .await
            .expect("due");
        assert!(!due.contains(&rid), "not due yet");
        let mut backdate = redis::cmd("ZADD");
        backdate
            .arg(keys::stream_cleanup_zset_key())
            .arg(1)
            .arg(&rid);
        let _: () = backdate.query_async(&mut conn).await.expect("backdate");
        let due = due_runners_for_stream_cleanup(Some(&client))
            .await
            .expect("due");
        assert!(due.contains(&rid), "due after backdate");
        remove_stream_cleanup_marker(Some(&client), &rid)
            .await
            .expect("remove");
        let due = due_runners_for_stream_cleanup(Some(&client))
            .await
            .expect("due");
        assert!(!due.contains(&rid), "gone after remove");

        // Stream deletion drops both keys.
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        raw_xadd(
            &mut conn,
            &keys::offline_stream_key(&rid),
            "*",
            "m",
            "config_push",
            "{}",
        )
        .await;
        delete_runner_stream(Some(&client), &rid)
            .await
            .expect("delete");
        assert_eq!(raw_exists(&mut conn, &keys::stream_key(&rid)).await, 0);
        assert_eq!(
            raw_exists(&mut conn, &keys::offline_stream_key(&rid)).await,
            0
        );
        cleanup(&client, &rid, &[]).await;
    }

    #[tokio::test]
    async fn live_reap_roundtrip() {
        let client = live_client();
        let runner = runner_settings();
        let rid = fresh_id();
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        for consumer in ["consumer-a", "consumer-keep", "consumer-pend"] {
            let mut create = redis::cmd("XGROUP");
            create
                .arg("CREATECONSUMER")
                .arg(keys::stream_key(&rid))
                .arg(keys::group_name(&rid))
                .arg(consumer);
            let _: () = create.query_async(&mut conn).await.expect("mkconsumer");
        }
        raw_xadd(
            &mut conn,
            &keys::stream_key(&rid),
            "*",
            "m",
            "config_push",
            "{}",
        )
        .await;
        // One pending entry guards consumer-pend.
        let mut read = redis::cmd("XREADGROUP");
        read.arg("GROUP")
            .arg(keys::group_name(&rid))
            .arg("consumer-pend")
            .arg("COUNT")
            .arg(1)
            .arg("STREAMS")
            .arg(keys::stream_key(&rid))
            .arg(">");
        let _: redis::Value = read.query_async(&mut conn).await.expect("seed pel");

        let mut keep = HashSet::new();
        keep.insert("consumer-keep".to_string());
        let reaped = reap_idle_consumers(Some(&client), &runner, &rid, &keep, Some(0))
            .await
            .expect("reap");
        assert_eq!(reaped, 0, "sums pending counts, not removals");
        let mut xinfo = redis::cmd("XINFO");
        xinfo
            .arg("CONSUMERS")
            .arg(keys::stream_key(&rid))
            .arg(keys::group_name(&rid));
        let reply: redis::Value = xinfo.query_async(&mut conn).await.expect("xinfo");
        let names = consumer_names(&reply);
        assert!(
            !names.contains(&"consumer-a".to_string()),
            "stale consumer reaped: {names:?}"
        );
        assert!(names.contains(&"consumer-keep".to_string()));
        assert!(names.contains(&"consumer-pend".to_string()));
        cleanup(&client, &rid, &[]).await;
    }

    #[tokio::test]
    async fn live_trim_small_single_node_trims_zero() {
        // Mirror of fixture `safe_trim_inside_floor`: MINID ~ on a
        // 3-entry single node trims 0 and keeps everything.
        let client = live_client();
        let rid = fresh_id();
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        for id in ["1000-0", "1000-1", "1000-2"] {
            raw_xadd(
                &mut conn,
                &keys::stream_key(&rid),
                id,
                &format!("m{id}"),
                "config_push",
                "{}",
            )
            .await;
        }
        let mut read = redis::cmd("XREADGROUP");
        read.arg("GROUP")
            .arg(keys::group_name(&rid))
            .arg("consumer-b")
            .arg("COUNT")
            .arg(10)
            .arg("STREAMS")
            .arg(keys::stream_key(&rid))
            .arg(">");
        let _: redis::Value = read.query_async(&mut conn).await.expect("seed pel");
        ack_for_session(
            Some(&client),
            &rid,
            &["1000-0".to_string(), "1000-1".to_string()],
        )
        .await
        .expect("ack");

        let trimmed = safe_trim_runner_stream(Some(&client), &rid, "2000-0")
            .await
            .expect("trim");
        assert_eq!(trimmed, Some(0));
        let mut xrange = redis::cmd("XRANGE");
        xrange.arg(keys::stream_key(&rid)).arg("-").arg("+");
        let reply: redis::Value = xrange.query_async(&mut conn).await.expect("xrange");
        assert_eq!(
            xrange_ids(&reply),
            vec![
                "1000-0".to_string(),
                "1000-1".to_string(),
                "1000-2".to_string()
            ]
        );
        cleanup(&client, &rid, &[]).await;
    }

    #[tokio::test]
    async fn live_trim_bulk_approximate() {
        // 300 entries, all but the last 10 acked: the ~ trim drops
        // whole nodes below the floor and keeps every id >= the
        // min pending.
        let client = live_client();
        let rid = fresh_id();
        ensure_stream_group(Some(&client), &rid)
            .await
            .expect("ensure");
        let mut conn = live_conn(&client).await;
        for seq in 1..=300 {
            raw_xadd(
                &mut conn,
                &keys::stream_key(&rid),
                &format!("{seq}-0"),
                "m",
                "config_push",
                "{}",
            )
            .await;
        }
        let mut read = redis::cmd("XREADGROUP");
        read.arg("GROUP")
            .arg(keys::group_name(&rid))
            .arg("consumer-c")
            .arg("COUNT")
            .arg(300)
            .arg("STREAMS")
            .arg(keys::stream_key(&rid))
            .arg(">");
        let _: redis::Value = read.query_async(&mut conn).await.expect("seed pel");
        let acked: Vec<String> = (1..=290).map(|seq| format!("{seq}-0")).collect();
        assert_eq!(
            ack_for_session(Some(&client), &rid, &acked)
                .await
                .expect("ack"),
            290
        );

        let trimmed = safe_trim_runner_stream(Some(&client), &rid, "9999-0")
            .await
            .expect("trim");
        let trimmed = trimmed.expect("trim runs");
        assert!(trimmed > 0, "bulk trim removes whole nodes");
        let mut xrange = redis::cmd("XRANGE");
        xrange.arg(keys::stream_key(&rid)).arg("-").arg("+");
        let reply: redis::Value = xrange.query_async(&mut conn).await.expect("xrange");
        let remaining = xrange_ids(&reply);
        assert_eq!(trimmed, 300 - remaining.len(), "count is exact");
        for seq in 291..=300 {
            assert!(
                remaining.contains(&format!("{seq}-0")),
                "ids >= min pending kept"
            );
        }
        cleanup(&client, &rid, &[]).await;
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
        // Faithful `runner_session` shape (models COLUMNS + types);
        // the lookup only reads id/runner_id/revoked_at/created_at.
        sqlx::query(
            "CREATE TEMPORARY TABLE runner_session (\
                id UUID PRIMARY KEY, \
                runner_id UUID NOT NULL, \
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
        runner_id: uuid::Uuid,
        revoked: bool,
    ) {
        sqlx::query(
            "INSERT INTO runner_session \
                (id, runner_id, protocol_version, created_at, last_seen_at, revoked_at, revoked_reason) \
             VALUES ($1, $2, 4, now(), now(), CASE WHEN $3 THEN now() ELSE NULL END, \
                CASE WHEN $3 THEN 'evicted' ELSE NULL END)",
        )
        .bind(id)
        .bind(runner_id)
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
        let rid = uuid::Uuid::new_v4();
        assert_eq!(
            active_session_id_for_runner(&mut *tx, rid)
                .await
                .expect("lookup"),
            None
        );
        let active = uuid::Uuid::new_v4();
        seed_session(&mut tx, uuid::Uuid::new_v4(), rid, true).await;
        seed_session(&mut tx, active, rid, false).await;
        assert_eq!(
            active_session_id_for_runner(&mut *tx, rid)
                .await
                .expect("lookup"),
            Some(active)
        );
        // A revoked-only runner reads as offline.
        let ghost = uuid::Uuid::new_v4();
        seed_session(&mut tx, uuid::Uuid::new_v4(), ghost, true).await;
        assert_eq!(
            active_session_id_for_runner(&mut *tx, ghost)
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
        let mut tx = live_tx(&pool).await;

        // Live branch: an active session row routes to the stream.
        let live_rid = uuid::Uuid::new_v4();
        seed_session(&mut tx, uuid::Uuid::new_v4(), live_rid, false).await;
        let stream_id = enqueue_for_runner(
            Some(&client),
            &mut *tx,
            &runner,
            live_rid,
            &message("config_push"),
        )
        .await
        .expect("enqueue live")
        .expect("live returns a stream id");
        assert!(stream_id.contains('-'), "stream id shape: {stream_id}");
        let mut conn = live_conn(&client).await;
        assert_eq!(
            raw_xlen(&mut conn, &keys::stream_key(&live_rid.to_string())).await,
            1
        );
        let live_key = keys::stream_key(&live_rid.to_string());

        // Offline-buffer branch: no row, queueable type.
        let off_rid = uuid::Uuid::new_v4();
        let buffered = enqueue_for_runner(
            Some(&client),
            &mut *tx,
            &runner,
            off_rid,
            &message("config_push"),
        )
        .await
        .expect("enqueue offline");
        assert_eq!(buffered, None);
        let off_key = keys::offline_stream_key(&off_rid.to_string());
        assert_eq!(raw_xlen(&mut conn, &off_key).await, 1);
        let mut ttl = redis::cmd("TTL");
        ttl.arg(&off_key);
        let ttl: i64 = ttl.query_async(&mut conn).await.expect("ttl");
        assert!(
            (86399..=86400).contains(&ttl),
            "buffer ttl is the 86400 default: {ttl}"
        );

        // Offline-reject branch: no row, `assign` raises with the
        // exact `RunnerOfflineError` text.
        let error = enqueue_for_runner(
            Some(&client),
            &mut *tx,
            &runner,
            off_rid,
            &message("assign"),
        )
        .await
        .expect_err("assign rejects offline");
        assert_eq!(
            error.to_string(),
            format!("runner {off_rid} is offline; type 'assign' cannot queue")
        );

        // Unknown types raise before any I/O even with live remotes.
        let error =
            enqueue_for_runner(Some(&client), &mut *tx, &runner, live_rid, &message("nope"))
                .await
                .expect_err("unknown type raises");
        assert_eq!(error.to_string(), "unknown message type 'nope'");

        // Reject/unknown paths enqueue nothing.
        assert_eq!(raw_xlen(&mut conn, &live_key).await, 1);
        assert_eq!(raw_xlen(&mut conn, &off_key).await, 1);
        cleanup(&client, &live_rid.to_string(), &[]).await;
        cleanup(&client, &off_rid.to_string(), &[]).await;
    }
}
