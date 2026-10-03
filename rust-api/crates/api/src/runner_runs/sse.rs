//! Chat SSE stream (D-15 L8): `GET
//! /api/runners/chat/sessions/<session_id>/events/`
//! (`views/chat.py:781-857`, `runner/web_urls.py` runner-chat-events).
//! Fixture: FX-RUN-09 `sse` section; the `#[cfg(test)]` suite replays
//! its frames byte-for-byte via `include_str!`.
//!
//! [`chat_event_stream`] is a plain-function view, not DRF: session
//! auth answers 403 JSON (`JsonResponse`, spaced separators — verified
//! live against Django 6.0.5, unlike the compact DRF bodies elsewhere
//! in this module), and unknown/unreadable sessions answer 404 JSON.
//! The stream subscribes to the L1 event channel *before* replaying
//! (`chat.py:813-815` order), replays `seq > after` uncapped, then
//! relays live publishes with `id:` tracking, skipping malformed and
//! invalid-seq payloads, and emits `: heartbeat` every 15 idle seconds.
//! Streaming failures truncate (Django sends `response.start` before
//! iterating, so a mid-stream failure is still a 200): missing Redis,
//! a failed subscribe, or a failed replay all end the body instead of
//! 500ing, exactly like the [`crate::assistant::events`] precedent.

use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use pidash_services::runner_runs::chat::{dumps_value, serialize_event_json, EventParts};
use pidash_types::runner_runs::consts::event_channel;

use super::chat::{can_read_session, fetch_session_with_runner};
use super::run_endpoints::py_int;
use super::{json_response, pool_of, py_truthy, server_error};
use crate::assistant::common::py_iso;
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// 403 body (`JsonResponse({"error": "authentication required"})`,
/// `chat.py:791-794`). Spaced: `JsonResponse` keeps the `json.dumps`
/// defaults (verified live), unlike the compact DRF bodies.
pub const SSE_AUTH_REQUIRED_BODY: &str = r#"{"error": "authentication required"}"#;

/// 404 body (`chat.py:798`). Spaced for the same reason — not the
/// compact [`super::chat::CHAT_NOT_FOUND_BODY`].
pub const SSE_NOT_FOUND_BODY: &str = r#"{"error": "not found"}"#;

/// Idle frame (`chat.py:837`).
pub const HEARTBEAT_FRAME: &str = ": heartbeat\n\n";

/// Heartbeat interval (`chat.py:835`): >= 15 monotonic seconds.
const HEARTBEAT_SECS: u64 = 15;

/// Live-tail poll (`chat.py:820`): `get_message(timeout=1.0)`.
const POLL_SECS: u64 = 1;

/// Replay row: `id, message_id, seq, kind, payload::text, created_at`.
type ReplayRow = (i64, Option<Uuid>, i32, String, String, DateTime<Utc>);

/// `after` cursor (`chat.py:800-804`): `GET.after or Last-Event-ID or
/// "0"` — empty strings fall through to the next source — then
/// `int()`, whose failure is 0. The `str` coercion reuses [`py_int`],
/// so the module's documented approximation applies (no `1_0`
/// underscores, `i64` range; both fall to 0 here instead of 500).
pub fn parse_after(after_qs: Option<&str>, last_event_id: Option<&str>) -> i64 {
    let raw = after_qs
        .filter(|text| !text.is_empty())
        .or_else(|| last_event_id.filter(|text| !text.is_empty()))
        .unwrap_or("0");
    py_int(&Value::String(raw.to_owned())).unwrap_or(0)
}

/// One event frame (`chat.py:818/834`):
/// `event: chat.event\nid: {seq}\ndata: {json}\n\n`.
pub fn chat_frame(seq: i64, event_json: &str) -> String {
    format!("event: chat.event\nid: {seq}\ndata: {event_json}\n\n")
}

/// What one live publish does (`chat.py:821-834`).
pub enum LiveAction {
    /// `seq > last_seq`: emit with the parsed `seq` as the frame id.
    Emit {
        /// The parsed publish `seq`.
        seq: i64,
        /// The publish re-dumped with `json.dumps` separators.
        data_json: String,
    },
    /// `seq <= last_seq`: silent, but execution falls through to the
    /// heartbeat check (no `continue` in Python).
    Stale,
    /// `json.loads` failed (`chat.py:824-826`): warn and `continue`,
    /// skipping the heartbeat check that iteration.
    Malformed,
    /// `int(seq)` failed (`chat.py:829-831`): warn and `continue`.
    InvalidSeq,
    /// The payload is not a dict: `.get` raises `AttributeError`,
    /// which neither `except` catches, so the generator dies and the
    /// stream truncates after the frames already sent.
    End,
}

/// Classify one live publish (`chat.py:821-834`).
///
/// `msg.get("data") or "{}"` means an empty payload parses as `{}` —
/// whose missing `seq` (`None or 0`) is 0, emitted only when
/// `0 > last_seq` (a negative `after` with an empty replay). The
/// caller logs the warn/error lines; this stays pure for the fixture
/// replay tests.
pub fn classify_publish(payload: &[u8], last_seq: i64) -> LiveAction {
    let text = if payload.is_empty() {
        b"{}".as_slice()
    } else {
        payload
    };
    let data: Value = match serde_json::from_slice(text) {
        Ok(data) => data,
        Err(_) => return LiveAction::Malformed,
    };
    let obj = match data.as_object() {
        Some(obj) => obj,
        None => return LiveAction::End,
    };
    // `int(data.get("seq") or 0)`: falsy values never reach `int()`.
    let seq = match obj.get("seq").filter(|value| py_truthy(value)) {
        None => 0,
        Some(raw) => match py_int(raw) {
            Ok(seq) => seq,
            Err(_) => return LiveAction::InvalidSeq,
        },
    };
    if seq > last_seq {
        // `json.dumps(data, default=str)`: `default` never fires on a
        // `json.loads` product; key order is the payload's (the api
        // crate builds `serde_json` with `preserve_order`).
        LiveAction::Emit {
            seq,
            data_json: dumps_value(&data),
        }
    } else {
        LiveAction::Stale
    }
}

fn sse_response(body: Body) -> Response {
    // Bare `StreamingHttpResponse(_events(),
    // content_type="text/event-stream")` (`chat.py:857`): no
    // cache-control / buffering headers — those belong to the
    // assistant stream's source, not this one.
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("chat SSE response builds")
}

/// A 200 stream that ends immediately: the truncate path for
/// subscribe-before-first-byte failures (`chat.py:813` order means no
/// replay prefix exists yet, unlike the assistant's replay-first
/// stream).
fn empty_stream() -> Response {
    sse_response(Body::from(""))
}

fn sse_not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, SSE_NOT_FOUND_BODY.to_owned())
}

/// `GET /api/runners/chat/sessions/<session_id>/events/`
/// (`chat.py:781-857`).
pub async fn chat_event_stream(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    // `_session_authenticates` (`chat.py:790-794`): session-only auth
    // (mirrored by `resolve_actor`), anonymous answers 403.
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let actor = match crate::license::resolve_actor(&pool, secret.as_bytes(), extension).await {
        Ok(actor) => actor,
        Err(_) => return server_error(),
    };
    let Some(actor) = actor else {
        return json_response(StatusCode::FORBIDDEN, SSE_AUTH_REQUIRED_BODY.to_owned());
    };
    // Session fetch + `can_read_chat` (`chat.py:796-798`): missing,
    // unreadable, and non-UUID ids all answer 404. (Django's
    // `<uuid:>` converter rejects non-UUIDs at the resolver with its
    // HTML 404; the module answers the endpoint JSON instead, as the
    // sibling web views do.)
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return sse_not_found(),
    };
    let fetched = match fetch_session_with_runner(&pool, session_id).await {
        Ok(fetched) => fetched,
        Err(response) => return response,
    };
    let Some((session, runner)) = fetched else {
        return sse_not_found();
    };
    if !can_read_session(&pool, actor.id, &session, &runner).await {
        return sse_not_found();
    }

    let after = parse_after(
        params.get("after").map(String::as_str),
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok()),
    );

    // Subscribe before replay (`chat.py:813-815` order). A failure
    // here precedes the first byte, but the response already started,
    // so the stream truncates to the empty body.
    let channel = event_channel(&session_id.to_string());
    let Some(redis) = state.redis().cloned() else {
        return empty_stream();
    };
    let mut pubsub = match redis.subscribe(&channel).await {
        Ok(pubsub) => pubsub,
        Err(error) => {
            tracing::error!(%error, session_id = %session_id, "chat SSE subscribe failed");
            return empty_stream();
        }
    };

    // Replay (`chat.py:815-818`): `seq > after`, ordered, uncapped.
    // A replay failure truncates like any streaming failure.
    let rows: Vec<ReplayRow> = match sqlx::query_as(
        r#"SELECT "id", "message_id", "seq", "kind", "payload"::text, "created_at"
           FROM "agent_chat_event" WHERE "session_id" = $1 AND "seq" > $2 ORDER BY "seq" ASC"#,
    )
    .bind(session_id)
    .bind(after)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, session_id = %session_id, "chat SSE replay failed");
            let _ = pubsub.unsubscribe(channel.as_str()).await;
            return empty_stream();
        }
    };
    let session_str = session_id.to_string();
    let mut prefix = String::new();
    let mut last_seq = after;
    for (id, message_id, seq, kind, payload_text, created_at) in &rows {
        let payload: Value = serde_json::from_str(payload_text).unwrap_or(Value::Null);
        let payload_json = dumps_value(&payload);
        let message_str = message_id.map(|id| id.to_string());
        let created = py_iso(created_at);
        let data = serialize_event_json(&EventParts {
            id: *id,
            session_id: &session_str,
            message_id: message_str.as_deref(),
            seq: *seq,
            kind,
            payload_json: &payload_json,
            created_at: &created,
        });
        last_seq = last_seq.max(*seq as i64);
        prefix.push_str(&chat_frame(*seq as i64, &data));
    }

    // Live tail (`chat.py:819-837`): `get_message(timeout=1.0)` is the
    // timeout around `RedisHandle::next_payload`, whose stream never
    // surfaces subscribe confirmations (the
    // `ignore_subscribe_messages=True` equivalent). A transport error
    // ends the feeder like Python's `except Exception`, and a dropped
    // receiver (client gone) ends it through the send failure. The
    // `finally` half unsubscribes, logging failures (`chat.py:839-847`);
    // dropping the `PubSub` closes the connection (`aclose`), and
    // closing idle Postgres connections is Django-specific (sqlx holds
    // none per stream).
    let (mut sender, body) = crate::sse_body::sse_channel();
    tokio::spawn(async move {
        if sender.send_data(Bytes::from(prefix)).await.is_err() {
            unsubscribe(&mut pubsub, &channel, &session_str).await;
            return;
        }
        let mut last_heartbeat = std::time::Instant::now();
        loop {
            match tokio::time::timeout(
                Duration::from_secs(POLL_SECS),
                redis.next_payload(&mut pubsub),
            )
            .await
            {
                Ok(Ok(payload)) => {
                    match classify_publish(&payload, last_seq) {
                        LiveAction::Emit { seq, data_json } => {
                            last_seq = seq;
                            if sender
                                .send_data(Bytes::from(chat_frame(seq, &data_json)))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        LiveAction::Stale => {}
                        LiveAction::Malformed => {
                            tracing::warn!(
                                session_id = %session_str,
                                "ignoring malformed chat SSE payload"
                            );
                            continue;
                        }
                        LiveAction::InvalidSeq => {
                            tracing::warn!(
                                session_id = %session_str,
                                "ignoring chat SSE payload with invalid seq"
                            );
                            continue;
                        }
                        LiveAction::End => {
                            tracing::error!(
                                session_id = %session_str,
                                "non-dict chat SSE payload ends the stream"
                            );
                            break;
                        }
                    }
                    if heartbeat_due(&mut last_heartbeat)
                        && sender
                            .send_data(Bytes::from_static(HEARTBEAT_FRAME.as_bytes()))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
                Ok(Err(error)) => {
                    tracing::error!(%error, session_id = %session_str, "chat SSE tail ended");
                    break;
                }
                Err(_) => {
                    if heartbeat_due(&mut last_heartbeat)
                        && sender
                            .send_data(Bytes::from_static(HEARTBEAT_FRAME.as_bytes()))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
            }
        }
        unsubscribe(&mut pubsub, &channel, &session_str).await;
    });
    sse_response(body)
}

/// `: heartbeat` when >= 15s elapsed, resetting the clock
/// (`chat.py:835-837`).
fn heartbeat_due(last_heartbeat: &mut std::time::Instant) -> bool {
    if last_heartbeat.elapsed() >= Duration::from_secs(HEARTBEAT_SECS) {
        *last_heartbeat = std::time::Instant::now();
        true
    } else {
        false
    }
}

/// `finally: unsubscribe` (`chat.py:839-843`), failures logged.
async fn unsubscribe(pubsub: &mut redis::aio::PubSub, channel: &str, session_id: &str) {
    if let Err(error) = pubsub.unsubscribe(channel).await {
        tracing::error!(%error, session_id = %session_id, "failed to unsubscribe chat SSE pubsub");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-09-handlers-daemon.golden.json");

    fn section() -> Value {
        let fx: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        fx.get("sse").expect("sse section").clone()
    }

    fn frames(case: &Value) -> Vec<String> {
        case.get("frames")
            .expect("frames")
            .as_array()
            .expect("frames array")
            .iter()
            .map(|frame| frame.as_str().expect("frame str").to_owned())
            .collect()
    }

    #[test]
    fn auth_and_not_found_bodies_match_fixture() {
        let sse = section();
        assert_eq!(
            serde_json::from_str::<Value>(SSE_AUTH_REQUIRED_BODY).expect("403 parses"),
            sse["auth_403"]["body"],
        );
        assert_eq!(sse["auth_403"]["status"], 403);
        assert_eq!(
            serde_json::from_str::<Value>(SSE_NOT_FOUND_BODY).expect("404 parses"),
            sse["gone_404"]["body"],
        );
        assert_eq!(sse["gone_404"]["status"], 404);
        assert_eq!(sse["forbidden_404"]["body"], sse["gone_404"]["body"]);
        // Spaced `JsonResponse` separators, not the compact DRF body.
        assert!(SSE_AUTH_REQUIRED_BODY.contains(r#""error": ""#));
        assert!(SSE_NOT_FOUND_BODY.contains(r#""error": ""#));
    }

    #[test]
    fn after_cursor_follows_or_chain() {
        // Query beats header (`replay_after_beats_header` replays
        // nothing: its `after` is past every row).
        assert_eq!(parse_after(Some("2"), Some("9")), 2);
        assert_eq!(parse_after(Some("99"), Some("1")), 99);
        // Empty query falls through to the header.
        assert_eq!(parse_after(Some(""), Some("1")), 1);
        assert_eq!(parse_after(None, Some("1")), 1);
        // Empty header falls through to zero.
        assert_eq!(parse_after(Some(""), Some("")), 0);
        assert_eq!(parse_after(None, None), 0);
        // `int()` failure is zero (`replay_bad_after` replays all).
        assert_eq!(parse_after(Some("nope"), None), 0);
        assert_eq!(parse_after(Some(""), Some("nope")), 0);
        // Surrounding whitespace parses (`int()` strips).
        assert_eq!(parse_after(Some("  3  "), None), 3);
        assert_eq!(parse_after(Some("-5"), None), -5);
    }

    #[test]
    fn replay_frames_match_fixture_byte_for_byte() {
        let sse = section();
        let session_id = "0a4b4a35-efae-4d39-a9e1-713211c11d74";
        let cases = [
            (121_i64, 1_i32, "k1", 1, "2026-10-02T23:10:04.602929+00:00"),
            (122, 2, "k2", 2, "2026-10-02T23:10:04.605913+00:00"),
            (123, 3, "k3", 3, "2026-10-02T23:10:04.606590+00:00"),
        ];
        let expected = frames(&sse["replay_all"]);
        assert_eq!(expected.len(), 3);
        for (index, (id, seq, kind, n, created_raw)) in cases.iter().enumerate() {
            let created_at: DateTime<Utc> = created_raw.parse().expect("fixture timestamp");
            assert_eq!(py_iso(&created_at), *created_raw);
            let payload = serde_json::json!({"n": n});
            let data = serialize_event_json(&EventParts {
                id: *id,
                session_id,
                message_id: None,
                seq: *seq,
                kind,
                payload_json: &dumps_value(&payload),
                created_at: &py_iso(&created_at),
            });
            assert_eq!(chat_frame(*seq as i64, &data), expected[index]);
        }
        // `after=2` replays only seq 3; `Last-Event-ID: 1` replays 2-3.
        assert_eq!(frames(&sse["replay_after"]), expected[2..]);
        assert_eq!(frames(&sse["replay_lei"]), expected[1..]);
        assert!(frames(&sse["replay_after_beats_header"]).is_empty());
        assert_eq!(frames(&sse["replay_bad_after"]), expected);
        assert_eq!(sse["replay_all"]["content_type"], "text/event-stream");
    }

    #[test]
    fn live_frame_matches_fixture_byte_for_byte() {
        let sse = section();
        let payload = br#"{"seq": 9, "kind": "live", "payload": {}}"#;
        match classify_publish(payload, 3) {
            LiveAction::Emit { seq, data_json } => {
                assert_eq!(seq, 9);
                assert_eq!(chat_frame(seq, &data_json), frames(&sse["live"])[0]);
            }
            _ => panic!("live publish must emit"),
        }
        // The channel is the L1 event channel.
        let subscribed = sse["replay_all"]["pubsub"]["subscribed"][0]
            .as_str()
            .expect("channel");
        assert_eq!(
            subscribed,
            event_channel("0a4b4a35-efae-4d39-a9e1-713211c11d74")
        );
        assert_eq!(
            sse["replay_all"]["pubsub"]["unsubscribed"][0]
                .as_str()
                .expect("channel"),
            subscribed
        );
        assert_eq!(sse["replay_all"]["pubsub"]["closed"], true);
    }

    #[test]
    fn live_stale_is_silent_but_checks_heartbeat() {
        // `seq <= last_seq`: `Stale` (the handler falls through to the
        // heartbeat check instead of `continue`ing).
        assert!(matches!(
            classify_publish(br#"{"seq": 3, "kind": "k"}"#, 3),
            LiveAction::Stale
        ));
        assert!(matches!(
            classify_publish(br#"{"seq": 1, "kind": "k"}"#, 9),
            LiveAction::Stale
        ));
        // Missing `seq` is `None or 0` = 0: stale against any live
        // cursor, emitted only past a negative `after`.
        assert!(matches!(
            classify_publish(br#"{"kind": "k"}"#, 0),
            LiveAction::Stale
        ));
        match classify_publish(br#"{"kind": "k"}"#, -5) {
            LiveAction::Emit { seq, data_json } => {
                assert_eq!(seq, 0);
                assert_eq!(data_json, r#"{"kind": "k"}"#);
            }
            _ => panic!("seq 0 beats last_seq -5"),
        }
        // Empty payload parses as `{}` (`or "{}"`), then as above.
        assert!(matches!(classify_publish(b"", 0), LiveAction::Stale));
    }

    #[test]
    fn live_malformed_and_invalid_seq_are_ignored() {
        assert!(matches!(
            classify_publish(br#"not json"#, 0),
            LiveAction::Malformed
        ));
        assert!(matches!(
            classify_publish(br#"{"seq": "nope"}"#, 0),
            LiveAction::InvalidSeq
        ));
        assert!(matches!(
            classify_publish(br#"{"seq": [1]}"#, 0),
            LiveAction::InvalidSeq
        ));
        // Falsy `seq` values never reach `int()`: 0, never invalid.
        assert!(matches!(
            classify_publish(br#"{"seq": 0}"#, -1),
            LiveAction::Emit { seq: 0, .. }
        ));
        assert!(matches!(
            classify_publish(br#"{"seq": ""}"#, -1),
            LiveAction::Emit { seq: 0, .. }
        ));
        // Truthy JSON numbers truncate like `int()`.
        match classify_publish(br#"{"seq": 9.9}"#, 0) {
            LiveAction::Emit { seq, .. } => assert_eq!(seq, 9),
            _ => panic!("float seq truncates"),
        }
        match classify_publish(br#"{"seq": "11"}"#, 0) {
            LiveAction::Emit { seq, .. } => assert_eq!(seq, 11),
            _ => panic!("string seq parses"),
        }
    }

    #[test]
    fn live_non_dict_payload_ends_the_stream() {
        // `.get` on a non-dict raises `AttributeError`, uncaught.
        assert!(matches!(classify_publish(br#"[1, 2]"#, 0), LiveAction::End));
        assert!(matches!(classify_publish(br#"7"#, 0), LiveAction::End));
        assert!(matches!(classify_publish(br#""s""#, 0), LiveAction::End));
    }

    #[test]
    fn heartbeat_frame_matches_fixture() {
        let sse = section();
        let expected = frames(&sse["heartbeat"]);
        assert!(!expected.is_empty());
        for frame in &expected {
            assert_eq!(frame, HEARTBEAT_FRAME);
        }
        assert_eq!(HEARTBEAT_FRAME, ": heartbeat\n\n");
    }
}
