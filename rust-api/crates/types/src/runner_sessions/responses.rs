//! D-14 session response shapes: open-201, poll-200, error bodies.
//!
//! Port of the inline-JSON layer in `views/sessions.py:97-119`
//! (protocol-header 426), `:132-154` (open 403/409), `:210-213` (open
//! 503), `:266-285` (open 201 + welcome), `:288-312` (delete 403/204),
//! `:330-352, :407-412` (bookkeeping 409/503), `:560-601` (poll
//! 405/401/403/400/409/200), and the machine twins in
//! `views/machine_sessions.py:76-82` (open 403), `:121-131` (open
//! 201), `:141-159` (delete 403/204), `:180-194` (bookkeeping 409),
//! `:289-345` (poll 405/401/403/400/409/200).
//!
//! Each builder returns the exact wire bytes plus its status. Open /
//! delete shapes render through the DRF `Response` renderer (compact);
//! poll shapes through Django's `JsonResponse` (spaced) — see the
//! renderer notes on [`super`]. `server_time` crosses this boundary
//! already rendered (`timezone.now().isoformat()`:
//! microsecond-precision RFC 3339 with a numeric `+00:00` offset, i.e.
//! chrono's `to_rfc3339_opts(SecondsFormat::Micros, false)`); the
//! interval, protocol version and version strings are the caller's
//! settings values, passed in so this crate stays settings-free.
//!
//! Fixture replayed by the unit tests below:
//! `rust-api/fixtures/runner_sessions/fx-rses-02-shapes.json`
//! (FX-RSES-02).

use serde_json::{Map, Value};

use super::envelopes::DecodedMessage;
use super::{render_compact_utf8, render_spaced_ascii};

/// HTTP 200 — poll envelope.
pub const STATUS_OK: u16 = 200;
/// HTTP 201 — session opened.
pub const STATUS_CREATED: u16 = 201;
/// HTTP 204 — clean shutdown (empty body).
pub const STATUS_NO_CONTENT: u16 = 204;
/// HTTP 400 — poll body is not JSON.
pub const STATUS_BAD_REQUEST: u16 = 400;
/// HTTP 401 — bearer token rejected.
pub const STATUS_UNAUTHORIZED: u16 = 401;
/// HTTP 403 — authenticated id does not match the URL id.
pub const STATUS_FORBIDDEN: u16 = 403;
/// HTTP 405 — poll is POST-only.
pub const STATUS_METHOD_NOT_ALLOWED: u16 = 405;
/// HTTP 409 — project mismatch / evicted session.
pub const STATUS_CONFLICT: u16 = 409;
/// HTTP 426 — `X-Runner-Protocol-Version` below minimum.
pub const STATUS_UPGRADE_REQUIRED: u16 = 426;
/// HTTP 503 — runner rows lock/statement-timed-out.
pub const STATUS_SERVICE_UNAVAILABLE: u16 = 503;

/// A rendered response: status plus exact body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

fn compact(status: u16, body: Map<String, Value>) -> HttpResponse {
    HttpResponse {
        status,
        body: render_compact_utf8(&Value::Object(body)),
    }
}

fn spaced(status: u16, body: Map<String, Value>) -> HttpResponse {
    HttpResponse {
        status,
        body: render_spaced_ascii(&Value::Object(body)),
    }
}

/// `POST /runners/<rid>/sessions/` 201 body
/// (`views/sessions.py:266-285`).
///
/// Top-level key order is `session_id, welcome, resume_ack, redeliver`;
/// the welcome runs `type, rid, server_time, long_poll_interval_secs,
/// protocol_version` with `latest_runner_version` /
/// `min_runner_version` appended only when the corresponding setting is
/// non-empty. DRF-compact bytes.
#[allow(clippy::too_many_arguments)]
pub fn runner_open_201(
    session_id: &str,
    runner_id: &str,
    server_time: &str,
    long_poll_interval_secs: i64,
    protocol_version: i64,
    latest_runner_version: Option<&str>,
    min_runner_version: Option<&str>,
    resume_ack: Option<&Value>,
    redeliver: Option<&Value>,
) -> HttpResponse {
    let mut welcome = Map::with_capacity(7);
    welcome.insert("type".to_string(), Value::String("welcome".to_string()));
    welcome.insert("rid".to_string(), Value::String(runner_id.to_string()));
    welcome.insert(
        "server_time".to_string(),
        Value::String(server_time.to_string()),
    );
    welcome.insert(
        "long_poll_interval_secs".to_string(),
        Value::from(long_poll_interval_secs),
    );
    welcome.insert(
        "protocol_version".to_string(),
        Value::from(protocol_version),
    );
    if let Some(version) = latest_runner_version {
        welcome.insert(
            "latest_runner_version".to_string(),
            Value::String(version.to_string()),
        );
    }
    if let Some(version) = min_runner_version {
        welcome.insert(
            "min_runner_version".to_string(),
            Value::String(version.to_string()),
        );
    }
    let mut body = Map::with_capacity(4);
    body.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    body.insert("welcome".to_string(), Value::Object(welcome));
    body.insert(
        "resume_ack".to_string(),
        resume_ack.cloned().unwrap_or(Value::Null),
    );
    body.insert(
        "redeliver".to_string(),
        redeliver.cloned().unwrap_or(Value::Null),
    );
    compact(STATUS_CREATED, body)
}

/// `POST /dev-machines/<mid>/sessions/` 201 body
/// (`views/machine_sessions.py:121-131`).
///
/// Key order is `session_id, welcome`; the welcome runs `type,
/// dev_machine_id, server_time, long_poll_interval_secs,
/// protocol_version` and — unlike the runner twin — never carries
/// version keys. DRF-compact bytes.
pub fn machine_open_201(
    session_id: &str,
    dev_machine_id: &str,
    server_time: &str,
    long_poll_interval_secs: i64,
    protocol_version: i64,
) -> HttpResponse {
    let mut welcome = Map::with_capacity(5);
    welcome.insert("type".to_string(), Value::String("welcome".to_string()));
    welcome.insert(
        "dev_machine_id".to_string(),
        Value::String(dev_machine_id.to_string()),
    );
    welcome.insert(
        "server_time".to_string(),
        Value::String(server_time.to_string()),
    );
    welcome.insert(
        "long_poll_interval_secs".to_string(),
        Value::from(long_poll_interval_secs),
    );
    welcome.insert(
        "protocol_version".to_string(),
        Value::from(protocol_version),
    );
    let mut body = Map::with_capacity(2);
    body.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    body.insert("welcome".to_string(), Value::Object(welcome));
    compact(STATUS_CREATED, body)
}

/// Poll 200 envelope, shared by the runner and machine polls
/// (`views/sessions.py:595-601`, `views/machine_sessions.py:339-345`).
///
/// Key order is `messages, server_time, long_poll_interval_secs`.
/// `JsonResponse`-spaced bytes.
pub fn poll_200(
    messages: &[DecodedMessage],
    server_time: &str,
    long_poll_interval_secs: i64,
) -> HttpResponse {
    let mut body = Map::with_capacity(3);
    body.insert(
        "messages".to_string(),
        Value::Array(messages.iter().map(DecodedMessage::to_json_value).collect()),
    );
    body.insert(
        "server_time".to_string(),
        Value::String(server_time.to_string()),
    );
    body.insert(
        "long_poll_interval_secs".to_string(),
        Value::from(long_poll_interval_secs),
    );
    spaced(STATUS_OK, body)
}

/// 426 `protocol_version_unsupported` (runner open only — the machine
/// open performs no protocol check). `minimum` is
/// `settings.RUNNER_PROTOCOL_VERSION`. DRF-compact bytes.
pub fn protocol_version_unsupported_drf(minimum: i64) -> HttpResponse {
    let mut body = Map::with_capacity(2);
    body.insert(
        "error".to_string(),
        Value::String("protocol_version_unsupported".to_string()),
    );
    body.insert("minimum".to_string(), Value::from(minimum));
    compact(STATUS_UPGRADE_REQUIRED, body)
}

/// 403 `runner_id_mismatch` on the DRF open/delete endpoints.
pub fn runner_id_mismatch_drf() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "error".to_string(),
        Value::String("runner_id_mismatch".to_string()),
    );
    compact(STATUS_FORBIDDEN, body)
}

/// 403 `runner_id_mismatch` on the runner poll (`JsonResponse` bytes).
pub fn runner_id_mismatch_poll() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "error".to_string(),
        Value::String("runner_id_mismatch".to_string()),
    );
    spaced(STATUS_FORBIDDEN, body)
}

/// 403 `dev_machine_mismatch` on the DRF machine open/delete endpoints.
pub fn dev_machine_mismatch_drf() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "error".to_string(),
        Value::String("dev_machine_mismatch".to_string()),
    );
    compact(STATUS_FORBIDDEN, body)
}

/// 403 `dev_machine_mismatch` on the machine poll (`JsonResponse`
/// bytes). Also covers a missing/invalid machine token on the DRF
/// endpoints, which funnels through the same `_auth_dev_machine`
/// `None` path.
pub fn dev_machine_mismatch_poll() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "error".to_string(),
        Value::String("dev_machine_mismatch".to_string()),
    );
    spaced(STATUS_FORBIDDEN, body)
}

/// 409 `project_mismatch` on runner open (DRF-compact bytes).
pub fn project_mismatch_drf(expected: &str) -> HttpResponse {
    let mut body = Map::with_capacity(2);
    body.insert(
        "error".to_string(),
        Value::String("project_mismatch".to_string()),
    );
    body.insert("expected".to_string(), Value::String(expected.to_string()));
    compact(STATUS_CONFLICT, body)
}

/// 409 `session_evicted` on either poll (`JsonResponse` bytes).
///
/// `reason` is the session's `revoked_reason` when the row exists but
/// is revoked, and absent for a missing row or an eviction that lands
/// mid-wait.
pub fn session_evicted_poll(reason: Option<&str>) -> HttpResponse {
    let mut body = Map::with_capacity(2);
    body.insert(
        "error".to_string(),
        Value::String("session_evicted".to_string()),
    );
    if let Some(reason) = reason {
        body.insert("reason".to_string(), Value::String(reason.to_string()));
    }
    spaced(STATUS_CONFLICT, body)
}

/// 503 `runner_state_locked` on runner open (DRF-compact bytes).
pub fn runner_state_locked_drf() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "error".to_string(),
        Value::String("runner_state_locked".to_string()),
    );
    compact(STATUS_SERVICE_UNAVAILABLE, body)
}

/// 503 `runner_state_locked` on the runner poll (`JsonResponse` bytes).
pub fn runner_state_locked_poll() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "error".to_string(),
        Value::String("runner_state_locked".to_string()),
    );
    spaced(STATUS_SERVICE_UNAVAILABLE, body)
}

/// 400 poll body-not-JSON (`JsonResponse` bytes; lowercase `detail`, like DRF).
pub fn json_parse_error_poll() -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "detail".to_string(),
        Value::String("JSON parse error".to_string()),
    );
    spaced(STATUS_BAD_REQUEST, body)
}

/// 401 on the DRF open/delete endpoints: `{"detail": <detail>}` where
/// `detail` is the D-13 auth failure text (`access_token_malformed`,
/// `runner_id_mismatch`, `machine_token_invalid`).
pub fn unauthorized_drf(detail: &str) -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert("detail".to_string(), Value::String(detail.to_string()));
    compact(STATUS_UNAUTHORIZED, body)
}

/// 401 on either poll: same shape through `JsonResponse`.
pub fn unauthorized_poll(detail: &str) -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert("detail".to_string(), Value::String(detail.to_string()));
    spaced(STATUS_UNAUTHORIZED, body)
}

/// 405 on the DRF open/delete routes: DRF's
/// `Method "<method>" not allowed.` detail, compact bytes.
pub fn method_not_allowed_drf(method: &str) -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "detail".to_string(),
        Value::String(format!("Method \"{method}\" not allowed.")),
    );
    compact(STATUS_METHOD_NOT_ALLOWED, body)
}

/// 405 on either poll: same text through `JsonResponse`.
pub fn method_not_allowed_poll(method: &str) -> HttpResponse {
    let mut body = Map::with_capacity(1);
    body.insert(
        "detail".to_string(),
        Value::String(format!("Method \"{method}\" not allowed.")),
    );
    spaced(STATUS_METHOD_NOT_ALLOWED, body)
}

/// 204 clean shutdown: status only, empty body.
pub fn no_content() -> HttpResponse {
    HttpResponse {
        status: STATUS_NO_CONTENT,
        body: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-02-shapes.json");

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

    fn keys_of(map: &Map<String, Value>) -> Vec<String> {
        map.keys().cloned().collect()
    }

    /// `runner_open_201`, replayed byte-exact: every input comes from the
    /// recorded sample (`session_id_is_uuid4`, `welcome_sample`,
    /// null `resume_ack`/`redeliver`).
    #[test]
    fn runner_open_201_replays_fixture() {
        let fx = fixture();
        let sample = fx.get("runner_open_201").expect("sample");
        let welcome = sample.get("welcome_sample").expect("welcome");
        let out = runner_open_201(
            sample["session_id_is_uuid4"].as_str().expect("sid"),
            welcome["rid"].as_str().expect("rid"),
            welcome["server_time"].as_str().expect("server_time"),
            welcome["long_poll_interval_secs"]
                .as_i64()
                .expect("interval"),
            welcome["protocol_version"].as_i64().expect("version"),
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            out.status,
            sample["status"].as_u64().expect("status") as u16
        );
        assert_eq!(
            out.body,
            sample["body_bytes_sample"].as_str().expect("bytes")
        );
        let parsed: Value = serde_json::from_str(&out.body).expect("parses");
        assert_eq!(
            keys_of(parsed.as_object().expect("object")),
            as_str_list(sample.get("key_order").expect("key_order"))
        );
        assert_eq!(
            keys_of(parsed["welcome"].as_object().expect("welcome")),
            as_str_list(sample.get("welcome_key_order").expect("welcome order"))
        );

        // Full-dispatch wire bytes: inputs parsed back out of the bytes.
        let wire = fx.get("open_201_dispatch_wire").expect("wire");
        let bytes = wire
            .get("body_bytes")
            .and_then(Value::as_str)
            .expect("bytes");
        let parsed: Value = serde_json::from_str(bytes).expect("parses");
        let out = runner_open_201(
            parsed["session_id"].as_str().expect("sid"),
            parsed["welcome"]["rid"].as_str().expect("rid"),
            parsed["welcome"]["server_time"]
                .as_str()
                .expect("server_time"),
            parsed["welcome"]["long_poll_interval_secs"]
                .as_i64()
                .expect("interval"),
            parsed["welcome"]["protocol_version"]
                .as_i64()
                .expect("version"),
            None,
            None,
            None,
            None,
        );
        assert_eq!(out.status, wire["status"].as_u64().expect("status") as u16);
        assert_eq!(out.body, bytes);
    }

    #[test]
    fn runner_open_201_version_keys_replay_fixture() {
        let fx = fixture();
        let sample = fx.get("runner_open_201_versions").expect("sample");
        let welcome = sample.get("welcome").expect("welcome");
        let out = runner_open_201(
            "sid",
            welcome["rid"].as_str().expect("rid"),
            welcome["server_time"].as_str().expect("server_time"),
            welcome["long_poll_interval_secs"]
                .as_i64()
                .expect("interval"),
            welcome["protocol_version"].as_i64().expect("version"),
            welcome["latest_runner_version"].as_str(),
            welcome["min_runner_version"].as_str(),
            None,
            None,
        );
        let parsed: Value = serde_json::from_str(&out.body).expect("parses");
        assert_eq!(parsed["welcome"], *welcome);
        assert_eq!(
            keys_of(parsed["welcome"].as_object().expect("welcome")),
            as_str_list(sample.get("welcome_key_order").expect("key order"))
        );
        // Absent settings omit both keys.
        let bare = runner_open_201("sid", "rid", "t", 25, 4, None, None, None, None);
        assert!(!bare.body.contains("latest_runner_version"));
        assert!(!bare.body.contains("min_runner_version"));
    }

    /// `machine_open_201`, replayed byte-exact: the session id is read
    /// back out of the recorded `INSERT` (the fixture stores no
    /// `body_bytes` for this one), everything else from `welcome_sample`.
    #[test]
    fn machine_open_201_replays_fixture() {
        let fx = fixture();
        let sample = fx.get("machine_open_201").expect("sample");
        let welcome = sample.get("welcome_sample").expect("welcome");
        let insert = sample["sql"]
            .as_array()
            .expect("sql")
            .iter()
            .find(|line| {
                line.as_str()
                    .is_some_and(|l| l.starts_with("INSERT INTO \"machine_session\""))
            })
            .and_then(Value::as_str)
            .expect("insert");
        let hex: String = insert
            .split("VALUES ('")
            .nth(1)
            .expect("values")
            .chars()
            .take_while(|c| *c != '\'')
            .collect();
        assert_eq!(hex.len(), 32);
        let sid = format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        );
        let out = machine_open_201(
            &sid,
            welcome["dev_machine_id"].as_str().expect("mid"),
            welcome["server_time"].as_str().expect("server_time"),
            welcome["long_poll_interval_secs"]
                .as_i64()
                .expect("interval"),
            welcome["protocol_version"].as_i64().expect("version"),
        );
        assert_eq!(
            out.status,
            sample["status"].as_u64().expect("status") as u16
        );
        let parsed: Value = serde_json::from_str(&out.body).expect("parses");
        assert_eq!(parsed["session_id"].as_str(), Some(sid.as_str()));
        assert_eq!(parsed["welcome"], *welcome);
        assert_eq!(
            keys_of(parsed.as_object().expect("object")),
            as_str_list(sample.get("key_order").expect("key_order"))
        );
        assert_eq!(
            keys_of(parsed["welcome"].as_object().expect("welcome")),
            as_str_list(sample.get("welcome_key_order").expect("welcome order"))
        );
        // Byte-exact composition of the two fixture facts above.
        assert_eq!(
            out.body,
            format!(
                "{{\"session_id\":\"{sid}\",\"welcome\":{{\"type\":\"welcome\",\"dev_machine_id\":\"e985a543-b79a-48a8-bdfe-7e0aaa937914\",\"server_time\":\"2026-10-02T22:22:34.218067+00:00\",\"long_poll_interval_secs\":25,\"protocol_version\":4}}}}"
            )
        );
    }

    #[test]
    fn poll_200_replays_fixture() {
        let fx = fixture();
        let sample = fx.get("poll_200_first").expect("sample");
        let out = poll_200(
            &[],
            sample["server_time_sample"].as_str().expect("server_time"),
            sample["long_poll_interval_secs"]
                .as_i64()
                .expect("interval"),
        );
        assert_eq!(
            out.status,
            sample["status"].as_u64().expect("status") as u16
        );
        assert_eq!(
            out.body,
            sample["body_bytes_sample"].as_str().expect("bytes")
        );
        let parsed: Value = serde_json::from_str(&out.body).expect("parses");
        assert_eq!(
            keys_of(parsed.as_object().expect("object")),
            as_str_list(sample.get("key_order").expect("key_order"))
        );
        // The blocking variant pins a non-default interval.
        let blocking = fx.get("poll_200_blocking_empty").expect("blocking");
        let out = poll_200(
            &[],
            "t",
            blocking["long_poll_interval_secs"]
                .as_i64()
                .expect("interval"),
        );
        let parsed: Value = serde_json::from_str(&out.body).expect("parses");
        assert_eq!(parsed["messages"], Value::Array(vec![]));
        assert_eq!(
            parsed["long_poll_interval_secs"],
            blocking["long_poll_interval_secs"]
        );
    }

    /// The two-step delivery entry, replayed through the decode →
    /// poll path: message fields come from the fixture, the envelope
    /// bytes are asserted exactly.
    #[test]
    fn poll_200_message_replays_fixture() {
        let fx = fixture();
        let entry = fx["poll_two_step_delivery"]["second"]["messages"][0].clone();
        let message = DecodedMessage {
            stream_id: entry["stream_id"].as_str().expect("stream_id").to_string(),
            mid: entry["mid"].as_str().expect("mid").to_string(),
            msg_type: entry["type"].as_str().expect("type").to_string(),
            body: entry["body"].clone(),
        };
        let out = poll_200(&[message], "2026-10-02T23:58:25.738360+00:00", 25);
        let parsed: Value = serde_json::from_str(&out.body).expect("parses");
        assert_eq!(parsed["messages"][0], entry);
        assert_eq!(
            out.body,
            "{\"messages\": [{\"stream_id\": \"1790985779441-0\", \"mid\": \"m-b3-1\", \"type\": \"config_push\", \"body\": {\"type\": \"config_push\", \"mid\": \"m-b3-1\"}}], \"server_time\": \"2026-10-02T23:58:25.738360+00:00\", \"long_poll_interval_secs\": 25}"
        );
        let ment = fx["machine_poll_two_step_delivery"]["second_messages"][0].clone();
        assert_eq!(ment["type"], Value::String("ping".to_string()));
    }

    /// Every parsed error body in the fixture: status + JSON shape.
    #[test]
    fn error_shapes_replay_fixture() {
        let fx = fixture();
        // 426: the header-check nulls (missing/blank/current/future) mean
        // "no error" — that logic is the handlers' (PIDASHCONV-557); the
        // shapes below are this module's.
        for label in ["old_1", "non_numeric", "via_open_old"] {
            let case = &fx["protocol_header"][label];
            let out = protocol_version_unsupported_drf(
                case["body"]["minimum"].as_i64().expect("minimum"),
            );
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
        // 403s on the DRF endpoints.
        for label in [
            "error_403_runner_id_mismatch",
            "error_403_no_auth",
            "runner_delete_403_no_auth",
        ] {
            let case = fx.get(label).expect("case");
            let out = runner_id_mismatch_drf();
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
        for label in ["other_machine", "no_token"] {
            let case = &fx["machine_open_403_mismatch"][label];
            let out = dev_machine_mismatch_drf();
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
        // 409 project mismatch.
        let case = fx.get("error_409_project_mismatch").expect("case");
        let out = project_mismatch_drf(case["body"]["expected"].as_str().expect("expected"));
        assert_eq!(out.status, 409);
        assert_eq!(
            serde_json::from_str::<Value>(&out.body).expect("parses"),
            case["body"]
        );
        // 503 on open.
        let case = fx.get("error_503_runner_state_locked").expect("case");
        let out = runner_state_locked_drf();
        assert_eq!(out.status, 503);
        assert_eq!(
            serde_json::from_str::<Value>(&out.body).expect("parses"),
            case["body"]
        );
        // 204s: status + empty bytes.
        for label in [
            "runner_delete_204",
            "runner_delete_204_missing",
            "machine_delete_204",
            "machine_delete_204_missing",
        ] {
            let case = fx.get(label).expect("case");
            let out = no_content();
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                out.body,
                case["body_bytes"].as_str().expect("bytes"),
                "{label}"
            );
        }
        // 405s (poll renders through JsonResponse).
        for label in ["poll_405_get", "machine_poll_405_get"] {
            let case = fx.get(label).expect("case");
            let out = method_not_allowed_poll("GET");
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
        // 400s.
        for label in ["poll_400_bad_json", "machine_poll_400_bad_json"] {
            let case = fx.get(label).expect("case");
            let out = json_parse_error_poll();
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
        // 403 on the polls.
        let case = fx.get("poll_403_url_mismatch").expect("case");
        let out = runner_id_mismatch_poll();
        assert_eq!(out.status, 403);
        assert_eq!(
            serde_json::from_str::<Value>(&out.body).expect("parses"),
            case["body"]
        );
        let case = fx.get("machine_poll_401_invalid_token").expect("case");
        let out = dev_machine_mismatch_poll();
        assert_eq!(out.status, case["status"].as_u64().expect("status") as u16);
        assert_eq!(
            serde_json::from_str::<Value>(&out.body).expect("parses"),
            case["body"]
        );
        // 409 evicted variants (reason present / absent).
        for (label, reason) in [
            ("poll_409_evicted", Some("evicted_by_new_session")),
            ("poll_409_evicted_during_wait", None),
        ] {
            let case = fx.get(label).expect("case");
            let out = session_evicted_poll(reason);
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
        for (label, reason) in [
            ("bookkeeping_missing_session", None),
            (
                "bookkeeping_revoked_session",
                Some("evicted_by_new_session"),
            ),
            ("machine_bookkeeping_missing", None),
        ] {
            let error = fx.get(label).and_then(|c| c.get("error")).expect("error");
            let out = session_evicted_poll(reason);
            assert_eq!(
                out.status,
                error["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                error["payload"],
                "{label}"
            );
        }
        // 401s on the polls (detail text comes from D-13 auth).
        for label in [
            "poll_401_invalid_token",
            "poll_401_cross_runner",
            "machine_poll_401_mt_invalid",
        ] {
            let case = fx.get(label).expect("case");
            let out = unauthorized_poll(case["body"]["detail"].as_str().expect("detail"));
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&out.body).expect("parses"),
                case["body"],
                "{label}"
            );
        }
    }

    /// Every recorded `body_bytes` error: byte-exact replay (inputs
    /// parsed back out of the bytes where the fixture stores outputs
    /// only).
    #[test]
    fn error_bytes_replay_fixture() {
        let fx = fixture();
        for label in [
            "open_401_dispatch",
            "open_get_bad_token",
            "delete_401_dispatch",
            "open_401_cross_runner",
            "machine_open_401_mt_invalid",
        ] {
            let case = fx.get(label).expect("case");
            let bytes = case
                .get("body_bytes")
                .and_then(Value::as_str)
                .expect("bytes");
            let parsed: Value = serde_json::from_str(bytes).expect("parses");
            let out = unauthorized_drf(parsed["detail"].as_str().expect("detail"));
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(out.body, bytes, "{label}");
        }
        let case = fx.get("open_405_dispatch_get").expect("case");
        let out = method_not_allowed_drf("GET");
        assert_eq!(out.status, 405);
        assert_eq!(out.body, case["body_bytes"].as_str().expect("bytes"));
        for label in [
            "machine_open_401_dispatch",
            "machine_open_403_non_mt_bearer",
        ] {
            let case = fx.get(label).expect("case");
            let out = dev_machine_mismatch_drf();
            assert_eq!(
                out.status,
                case["status"].as_u64().expect("status") as u16,
                "{label}"
            );
            assert_eq!(
                out.body,
                case["body_bytes"].as_str().expect("bytes"),
                "{label}"
            );
        }
        // Spaced/poll twins of the same shapes (no `body_bytes` recorded;
        // separators proved by the renderer differential + poll samples).
        assert_eq!(
            runner_id_mismatch_poll().body,
            "{\"error\": \"runner_id_mismatch\"}"
        );
        assert_eq!(
            dev_machine_mismatch_poll().body,
            "{\"error\": \"dev_machine_mismatch\"}"
        );
        assert_eq!(
            session_evicted_poll(Some("evicted_by_new_session")).body,
            "{\"error\": \"session_evicted\", \"reason\": \"evicted_by_new_session\"}"
        );
        assert_eq!(
            runner_state_locked_poll().body,
            "{\"error\": \"runner_state_locked\"}"
        );
        assert_eq!(
            json_parse_error_poll().body,
            "{\"detail\": \"JSON parse error\"}"
        );
    }
}
