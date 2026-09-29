//! Assistant SSE event-stream handler (D-06, stage 5).
//!
//! Port of `apps/api/pi_dash/assistant/views/events.py:1-116`
//! (`assistant_event_stream`, `urls.py:51-53`): a plain (non-DRF) async
//! view. Authentication resolves the session user, then the member gate,
//! then the owned-thread scope — denials are *empty* bodies (401 when
//! unauthenticated, 404 otherwise, `events.py:62-66`) via the
//! [`crate::assistant::perm`] decision. The stream replays persisted events
//! (`seq > after`, ordered, at most 1000) as
//! `event: chat.event\ndata: {...}\n\n` frames, then tails the thread's
//! Redis channel; an idle second yields `: keepalive`. Headers disable
//! proxy buffering and gzip (`Cache-Control: no-cache`,
//! `X-Accel-Buffering: no`, `Content-Encoding: identity`).
//!
//! The `after` cursor comes from the `?after=` query parameter
//! (`events.py:68`, lenient like the message list). When the query parameter
//! is absent, a `Last-Event-ID` request header is honored as the cursor —
//! the standard `EventSource` reconnect behavior this issue's spec names
//! ("SSE stream with Last-Event-ID replay"); Python only reads the query
//! parameter, and header-absent traffic renders byte-identically.
//!
//! Registration is the cutover granularity: the owned GET serves from Rust,
//! every other method on this path proxies to Django.
//!
//! Fixture id F-A6-06 (events/messages envelopes + seq).

// Every handler returns a fully-rendered `Response` by design (like the
// intake `parse_body` precedent, which carries per-function allows for the
// same lint).
#![allow(clippy::result_large_err)]

use std::collections::HashMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use pidash_db::assistant::event_queries::{serialize_event_json, EventParts};
use uuid::Uuid;

use super::common::{
    parse_after, pool_ref, py_iso, query_last, request_actor, role_for, server_error,
    thread_not_found, HandlerResult,
};
use super::redis::live_tail_channel;
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Register the owned SSE route. Unowned methods fall through to Django
/// through the edge fallback (never a Rust 405).
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/events/",
        get(assistant_event_stream)
            .post(crate::edge::proxy)
            .put(crate::edge::proxy)
            .patch(crate::edge::proxy)
            .delete(crate::edge::proxy)
            .options(crate::edge::proxy),
    )
}

/// Render a JSON value exactly like stdlib `json.dumps(value)`
/// (`ensure_ascii=True`, the `(', ', ': ')` separators): the spaced twin of
/// the compact DRF rendering, used for event payloads inside both the
/// replay frames and the publish path (`events.py:47,62`). String escaping
/// follows the `dumps_string` helper in `db::assistant::event_queries`
/// (restated here because that helper is private to the db crate); object
/// key order is the parse order (serde_json's `preserve_order`, matching
/// what Python parses out of the `jsonb` column text).
pub fn py_dumps(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(true) => "true".to_owned(),
        serde_json::Value::Bool(false) => "false".to_owned(),
        serde_json::Value::Number(number) => py_float(number),
        serde_json::Value::String(text) => dumps_string(text),
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_dumps).collect();
            format!("[{}]", parts.join(", "))
        }
        serde_json::Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", dumps_string(key), py_dumps(item)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Number rendering: integers verbatim; floats via serde_json (shortest
/// round-trip digits, which are unique) re-notated to CPython `repr` rules:
/// scientific notation iff the decimal exponent is `< -4` or `>= 16`
/// (`repr(0.0001)` is fixed, `repr(0.00001)` is `1e-05`; `repr(1e15)` is
/// fixed, `repr(1e16)` is `1e+16`), a `.0` suffix on integral fixed floats,
/// and a signed two-or-more-digit exponent. Ryū (serde_json) differs twice:
/// it keeps fixed notation down to `1e-7`-ish (`0.000015` vs `1.5e-05`)
/// and prints bare exponents (`1e16` vs `1e+16`) — both fixed here. `NaN`
/// and infinities cannot occur (`jsonb` rejects them, and `Value` cannot
/// hold them).
fn py_float(number: &serde_json::Number) -> String {
    let rendered = number.to_string();
    if rendered.contains('e') || rendered.contains('E') {
        return py_exponent(&rendered);
    }
    let (negative, digits) = match rendered.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, rendered.as_str()),
    };
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (digits, ""),
    };
    // Decimal exponent of the first significant digit.
    let exp10 = if !int_part.trim_start_matches('0').is_empty() {
        int_part.trim_start_matches('0').len() as i32 - 1
    } else {
        let leading = frac_part.chars().take_while(|ch| *ch == '0').count();
        if leading == frac_part.len() {
            // Zero (or "0.000"): Python renders `0.0`.
            return if negative { "-0.0".to_owned() } else { "0.0".to_owned() };
        }
        -(leading as i32 + 1)
    };
    if (-4..16).contains(&exp10) {
        return rendered;
    }
    // Scientific: significant digits without leading/trailing zeros.
    let mut significant: String = format!("{int_part}{frac_part}")
        .trim_start_matches('0')
        .to_string();
    while significant.ends_with('0') && significant.len() > 1 {
        significant.pop();
    }
    let mantissa = if significant.len() == 1 {
        significant
    } else {
        format!("{}.{}", &significant[..1], &significant[1..])
    };
    let prefix = if negative { "-" } else { "" };
    format!("{prefix}{mantissa}{}", py_exp_suffix(exp10))
}

/// Re-render a Ryū scientific float (`1e16`, `1.5e-5`, `2E+3`) with the
/// Python exponent: always signed, at least two digits.
fn py_exponent(rendered: &str) -> String {
    let (mantissa, exp) = rendered.split_once(['e', 'E']).expect("split on e");
    let (sign, digits) = match exp.strip_prefix('+') {
        Some(rest) => ("+", rest),
        None => match exp.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("+", exp),
        },
    };
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let padded = if digits.len() >= 2 {
        digits.to_owned()
    } else {
        format!("0{digits}")
    };
    format!("{mantissa}e{sign}{padded}")
}

/// Python exponent suffix for a decimal exponent (`+16` → `e+16`).
fn py_exp_suffix(exp10: i32) -> String {
    if exp10 >= 0 {
        format!("e+{exp10:02}")
    } else {
        format!("e-{exp10:02}", exp10 = -exp10)
    }
}

/// stdlib `json.dumps` string escaping (`ensure_ascii=True`): short escapes
/// for `"`, `\`, `\n`, `\r`, `\t`, `\b`, `\f`; `\u00xx` (lowercase) for
/// other controls and DEL; `\uXXXX` (lowercase, surrogate pairs past the
/// BMP) for everything non-ASCII.
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

/// One SSE data frame (`events.py:46-47`):
/// `event: chat.event\ndata: {json}\n\n`.
pub fn sse_frame(event_json: &str) -> String {
    format!("event: chat.event\ndata: {event_json}\n\n")
}

/// `assistant_event_stream` (`events.py:50-116`).
async fn assistant_event_stream(
    State(state): State<AppState>,
    Path((slug, thread_id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    // `_resolve` (`events.py:28-38`) as the ported pure decision: the actor
    // lookup doubles as the `is_authenticated` check (unknown/inactive
    // session users resolve to nobody, like DRF's session auth), and the
    // thread scope doubles as the thread fetch.
    let actor = request_actor(pool, extension).await?;
    let authenticated = actor.is_some();
    let user_id = actor.as_ref().map(|actor| actor.id);
    let role = match user_id {
        Some(id) => role_for(pool, &id, &slug).await?,
        None => None,
    };
    let thread_id: Uuid = thread_id.parse().map_err(|_| thread_not_found())?;
    let thread_found = match user_id {
        Some(id) => {
            super::common::owned_thread(pool, &thread_id, &id, &slug)
                .await?
                .is_some()
        }
        None => false,
    };
    let decision =
        crate::assistant::perm::resolve_sse(authenticated, role, thread_found);
    if let Some(rejection) = crate::assistant::perm::sse_rejection(decision) {
        return Ok(rejection);
    }

    // `after` cursor (`events.py:67-70`); the `Last-Event-ID` header feeds
    // the same cursor only when the query parameter is absent.
    let after = match query_last(&params, "after") {
        Some(raw) => parse_after(Some(&raw)),
        None => headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|text| text.trim().parse::<i64>().ok())
            .unwrap_or(0),
    };

    // Replay (`events.py:41-44,73-74`): `seq > after`, ordered, at most
    // 1000 rows. The payload crosses as the column's `::text`, parsed
    // order-preserving and re-rendered spaced (see `py_dumps`).
    // `message` is the event's nullable `message_id` UUIDField
    // (`events.py:44`), not the turn.
    let rows = sqlx::query_as::<_, (i64, Uuid, Option<Uuid>, i64, String, String, DateTime<Utc>)>(
        "SELECT \"id\", \"thread_id\", \"message_id\", \"seq\", \"kind\", \
         \"payload\"::text, \"created_at\" \
         FROM \"assistant_event\" \
         WHERE (\"thread_id\" = $1 AND \"seq\" > $2) \
         ORDER BY \"seq\" ASC LIMIT 1000",
    )
    .bind(thread_id)
    .bind(after)
    .fetch_all(pool)
    .await
    .map_err(|_| server_error())?;
    let mut frames = Vec::with_capacity(rows.len());
    for (id, row_thread_id, message_id, seq, kind, payload_text, created_at) in &rows {
        let payload: serde_json::Value =
            serde_json::from_str(payload_text).unwrap_or(serde_json::Value::Null);
        let thread_str = row_thread_id.to_string();
        let message_str = message_id.map(|id| id.to_string());
        let created = py_iso(created_at);
        frames.push(sse_frame(&serialize_event_json(&EventParts {
            id: *id,
            thread_id: &thread_str,
            message_id: message_str.as_deref(),
            seq: *seq,
            kind,
            payload_json: &py_dumps(&payload),
            created_at: &created,
        })));
    }

    // PENDING(redis-foundation): the finished stream feeds `frames`, then
    // the live tail, then 1s keepalives through a channel body
    // (`http_body_util::channel`, which needs its `channel` feature enabled
    // in `api/Cargo.toml` — a foundation-file change owned by the Redis
    // foundation issue, together with the shared client). The live tail
    // subscribes to `assistant:thread:<id>` (`events.py:77-79`, channel
    // builder [`live_tail_channel`]) and relays each publish verbatim
    // (`event: chat.event\ndata: <data>\n\n`, `events.py:86-89`); an idle
    // second yields `: keepalive` (`events.py:81-84`, constant
    // `redis::SSE_KEEPALIVE_FRAME`), and a dropped receiver (client gone)
    // ends the feeder task.
    //
    // Interim: the replay prefix as a finite body. Headers, auth, cursor,
    // and frame bytes are already exact, so the contract suite's
    // replay-prefix reads pass; the infinite tail (keepalives + live
    // events) arrives with the foundation issue, and this route does not
    // merge until then.
    let _ = live_tail_channel(&thread_id);
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        // `Cache-Control: no-cache`, `X-Accel-Buffering: no` (disable nginx
        // proxy buffering), `Content-Encoding: identity` (Django's
        // GZipMiddleware skip) — `events.py:109-115`.
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .header(header::CONTENT_ENCODING, "identity")
        .body(axum::body::Body::from(frames.join("")))
        .expect("SSE response builds");
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::redis::SSE_KEEPALIVE_FRAME;

    #[test]
    fn py_dumps_matches_stdlib_separators_and_escapes() {
        assert_eq!(py_dumps(&serde_json::json!(null)), "null");
        assert_eq!(py_dumps(&serde_json::json!(true)), "true");
        assert_eq!(py_dumps(&serde_json::json!(1)), "1");
        assert_eq!(py_dumps(&serde_json::json!(1.5)), "1.5");
        assert_eq!(py_dumps(&serde_json::json!("a\"b\nc")), r#""a\"b\nc""#);
        // Non-ASCII is \u-escaped (ensure_ascii=True).
        assert_eq!(py_dumps(&serde_json::json!("héllo")), r#""h\u00e9llo""#);
        assert_eq!(py_dumps(&serde_json::json!("𝄞")), r#""\ud834\udd1e""#);
        // Spaced separators, recursively; empties stay bare.
        assert_eq!(
            py_dumps(&serde_json::json!({"b": 1, "a": [1, 2]})),
            r#"{"b": 1, "a": [1, 2]}"#
        );
        assert_eq!(py_dumps(&serde_json::json!({})), "{}");
        assert_eq!(py_dumps(&serde_json::json!([])), "[]");
    }

    #[test]
    fn py_float_uses_python_exponent_form() {
        // Notation boundaries, oracle-verified against CPython `repr`.
        let cases: &[(f64, &str)] = &[
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1.5e-5, "1.5e-05"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (123.456, "123.456"),
            (100.0, "100.0"),
            (0.0, "0.0"),
            (-2.5, "-2.5"),
            (1.0, "1.0"),
        ];
        for (value, expected) in cases {
            let number = serde_json::Number::from_f64(*value).expect("finite");
            assert_eq!(py_float(&number), *expected, "value {value}");
        }
        assert_eq!(py_float(&serde_json::Number::from(7)), "7");
    }

    #[test]
    fn sse_frame_wraps_event_json() {
        assert_eq!(
            sse_frame(r#"{"seq": 1}"#),
            "event: chat.event\ndata: {\"seq\": 1}\n\n"
        );
        assert_eq!(SSE_KEEPALIVE_FRAME, ": keepalive\n\n");
    }
}
