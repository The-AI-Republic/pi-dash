//! Assistant throttle classes (D-06, stage 5).
//!
//! Ports the six `UserRateThrottle` subclasses of
//! `apps/api/pi_dash/assistant/` with their rates from
//! `pi_dash/settings/common.py:93-117` —
//!
//! | Scope | Rate | Class (source) |
//! | --- | --- | --- |
//! | `assistant_message` | 30/hour | `AssistantMessageThrottle`, POST-only (`views/messages.py:29-42`) |
//! | `assistant_llm_test` | 6/minute | `UserLLMConfigTestThrottle` (`views/llm_config.py:81-86`) |
//! | `assistant_llm_generate_title` | 20/minute | `AssistantGenerateTitleThrottle` (`views/llm_config.py:111-126`) |
//! | `assistant_stt_test` | 6/minute | `UserSTTConfigTestThrottle` (`views/stt_config.py:84-89`) |
//! | `assistant_transcribe` | 20/minute | `AssistantTranscribeThrottle` (`views/transcribe.py:57-62`) |
//! | `assistant_agent_token` | 12/minute | `AgentModelTokenThrottle` (`views/agent_profile.py:67-82`) |
//!
//! Fixture id F-A6-07 (`rust-api/fixtures/assistant/perms.json`, `throttles`
//! + `rates_source`).
//!
//! Shape of the port: DRF's sliding-window decision as pure functions over a
//! caller-held timestamp history, plus the per-endpoint scope mapping. The
//! cache itself (redis, `django_redis`) and the wiring stay with the handler
//! layer; this module pins the numbers and the algorithm so any wiring — a
//! `tower_governor`-style per-scope governor or the DRF cache loop — matches
//! Python exactly. Deliberately *not* a token bucket: DRF allows a burst of
//! the full quota inside any window (`len(history) < num_requests` after
//! trimming entries `<= now - duration`), which a bucket with trickle refill
//! would not reproduce.
//!
//! DRF semantics reproduced (`SimpleRateThrottle` / `UserRateThrottle`):
//!
//! * Rate strings parse as `<num>/<period>` with the period's first letter
//!   selecting seconds (`s/m/h/d` -> 1/60/3600/86400; `parse_rate`).
//! * Cache key `throttle_<scope>_<ident>` where `ident` is the authenticated
//!   user's pk, else the client IP (`get_cache_key`, `cache_format`).
//! * `allow_request`: trim history entries `<= now - duration`, allow iff
//!   fewer than `num_requests` remain, else deny. Success records `now` and
//!   re-caches with timeout `duration` (`throttle_success`).
//! * Denial raises `Throttled`, which the project's `auth_exception_handler`
//!   rewrites to 429 + `{"error_code":5900,"error_message":
//!   "RATE_LIMIT_EXCEEDED"}` (`authentication/adapter/exception.py:28-34`,
//!   `error.py:63,84-94`) — the DRF default detail (with its `wait` seconds)
//!   never reaches the client, so [`throttle_wait`] is recorded for
//!   completeness but not rendered.
//!
//! Endpoint notes (ported as written):
//!
//! * Messages are throttled on POST only: `get_throttles` returns the
//!   throttle for POST and the default set otherwise (`messages.py:39-42`).
//! * The agent-profile endpoint is unthrottled; only the token endpoint
//!   carries `AgentModelTokenThrottle` (`agent_profile.py:67-82`).
//! * Thread list/create/detail, cancel, and the SSE stream carry no throttle
//!   class — the 30/hour brake is the message-POST scope alone.
//! * Check order: DRF `initial()` runs `check_throttles` before the handler
//!   body, so a throttled caller answers 429 even when the member gate would
//!   also deny it (see [`crate::assistant::perm`]).

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// Exact bytes of a throttled assistant response: the `auth_exception_handler`
/// rewrite (`exception.py:28-34`) of DRF's `Throttled`, rendered compact.
/// `error_code` 5900 is `AUTHENTICATION_ERROR_CODES["RATE_LIMIT_EXCEEDED"]`
/// (`error.py:63`); verified against the live module in the PIDASHCONV-249
/// run (`AuthenticationException(...).get_error_dict()` + compact dump).
pub const RATE_LIMIT_BODY: &str = r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#;

/// DRF `cache_format`: `'throttle_%(scope)s_%(ident)s'`.
pub fn cache_key(scope: &str, ident: &str) -> String {
    format!("throttle_{scope}_{ident}")
}

/// One ported throttle class: its DRF `scope`, quota, window, and source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottleSpec {
    /// DRF `scope` class attribute (also the `DEFAULT_THROTTLE_RATES` key).
    pub scope: &'static str,
    /// Allowed requests per window (`parse_rate` numerator).
    pub requests: u32,
    /// Window length in seconds (`parse_rate` denominator).
    pub window_secs: u64,
}

/// `assistant_message`: 30/hour, message POST only (`messages.py:29-42`,
/// `settings/common.py:103`).
pub const MESSAGE_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "assistant_message",
    requests: 30,
    window_secs: 3600,
};
/// `assistant_llm_test`: 6/minute (`llm_config.py:81-86`,
/// `settings/common.py:104`).
pub const LLM_TEST_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "assistant_llm_test",
    requests: 6,
    window_secs: 60,
};
/// `assistant_stt_test`: 6/minute (`stt_config.py:84-89`,
/// `settings/common.py:105`).
pub const STT_TEST_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "assistant_stt_test",
    requests: 6,
    window_secs: 60,
};
/// `assistant_transcribe`: 20/minute (`transcribe.py:57-62`,
/// `settings/common.py:110`).
pub const TRANSCRIBE_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "assistant_transcribe",
    requests: 20,
    window_secs: 60,
};
/// `assistant_llm_generate_title`: 20/minute (`llm_config.py:111-126`,
/// `settings/common.py:111`).
pub const GENERATE_TITLE_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "assistant_llm_generate_title",
    requests: 20,
    window_secs: 60,
};
/// `assistant_agent_token`: 12/minute (`agent_profile.py:67-82`,
/// `settings/common.py:114`).
pub const AGENT_TOKEN_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "assistant_agent_token",
    requests: 12,
    window_secs: 60,
};

/// Every ported throttle class, in `settings/common.py` order.
pub const ALL_THROTTLES: &[ThrottleSpec] = &[
    MESSAGE_THROTTLE,
    LLM_TEST_THROTTLE,
    STT_TEST_THROTTLE,
    TRANSCRIBE_THROTTLE,
    GENERATE_TITLE_THROTTLE,
    AGENT_TOKEN_THROTTLE,
];

/// Throttled attach points: the six class applications plus the two
/// deliberate non-applications the contract suite pins (message GET and the
/// agent-profile endpoint are unthrottled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottlePoint {
    /// `POST threads/<id>/messages/` — the only throttled thread route.
    PostMessages,
    /// `POST users/me/ai-assistant/config/test/`.
    LlmConfigTest,
    /// `POST workspaces/<slug>/ai-assistant/generate-title/`.
    GenerateTitle,
    /// `POST users/me/ai-assistant/stt-config/test/`.
    SttConfigTest,
    /// `POST users/me/ai-assistant/transcribe/`.
    Transcribe,
    /// `POST users/me/ai-assistant/agent-token/`.
    AgentToken,
}

/// Map an attach point to its throttle class (`None` = unthrottled by design:
/// message GET, agent-profile GET, thread CRUD, cancel, SSE).
pub fn spec_for(point: ThrottlePoint) -> ThrottleSpec {
    match point {
        ThrottlePoint::PostMessages => MESSAGE_THROTTLE,
        ThrottlePoint::LlmConfigTest => LLM_TEST_THROTTLE,
        ThrottlePoint::GenerateTitle => GENERATE_TITLE_THROTTLE,
        ThrottlePoint::SttConfigTest => STT_TEST_THROTTLE,
        ThrottlePoint::Transcribe => TRANSCRIBE_THROTTLE,
        ThrottlePoint::AgentToken => AGENT_TOKEN_THROTTLE,
    }
}

/// Mirror of `AssistantMessageListCreateEndpoint.get_throttles`
/// (`messages.py:39-42`): the message scope applies to POST only.
/// `method` is the HTTP method (`"POST"`, `"GET"`, ...).
pub fn messages_spec_for_method(method: &str) -> Option<ThrottleSpec> {
    if method == "POST" {
        Some(MESSAGE_THROTTLE)
    } else {
        None
    }
}

/// Mirror of `SimpleRateThrottle.parse_rate`: `<num>/<period>` where the
/// period's first letter gives seconds (`s/m/h/d` -> 1/60/3600/86400).
/// Returns `None` for malformed input (Python raises there; the table below
/// is total, so `None` only fires on caller error).
pub fn parse_rate(rate: &str) -> Option<(u32, u64)> {
    let (num, period) = rate.split_once('/')?;
    let requests: u32 = num.parse().ok()?;
    let window_secs = match period.chars().next()? {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return None,
    };
    Some((requests, window_secs))
}

/// Mirror of `SimpleRateThrottle.allow_request` as a pure predicate:
/// `history` is the cached timestamp list (newest first, as `throttle_success`
/// stores it), `now` is `timer()` (`time.time`). Entries `<= now - duration`
/// have passed out of the window; the request is allowed iff fewer than
/// `num_requests` remain. On success the caller records `now` at the front
/// and re-caches with timeout `duration`.
pub fn allow_request(history: &[f64], now: f64, num_requests: u32, window_secs: u64) -> bool {
    let duration = window_secs as f64;
    let live = history.iter().filter(|&&t| t > now - duration).count();
    live < num_requests as usize
}

/// Mirror of `SimpleRateThrottle.wait`: recommended seconds before the next
/// request, over the *trimmed* history (`allow_request` sets `self.history`
/// before `check_throttles` reads `wait()`). `None` when no request is
/// available. Rendered nowhere — the 429 rewrite drops it — but kept so the
/// algorithm ports whole.
pub fn throttle_wait(
    trimmed_history: &[f64],
    now: f64,
    num_requests: u32,
    window_secs: u64,
) -> Option<f64> {
    let duration = window_secs as f64;
    let remaining = match trimmed_history.last() {
        Some(&oldest) => duration - (now - oldest),
        None => duration,
    };
    let available = num_requests as i64 - trimmed_history.len() as i64 + 1;
    if available <= 0 {
        return None;
    }
    Some(remaining / available as f64)
}

/// Rejection for a throttled request: answers 429 with the rewritten
/// `RATE_LIMIT_EXCEEDED` body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Throttled;

impl IntoResponse for Throttled {
    fn into_response(self) -> Response {
        Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(RATE_LIMIT_BODY))
            .expect("static throttled response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// F-A6-07 `throttles` + `rates_source`: every (scope, rate) pair matches
    /// `settings/common.py:93-117`, and each rate string re-parses to the
    /// table numbers through the ported `parse_rate`.
    #[test]
    fn all_rates_match_python_settings() {
        let expected: &[(&str, &str, u32, u64)] = &[
            ("assistant_message", "30/hour", 30, 3600),
            ("assistant_llm_test", "6/minute", 6, 60),
            ("assistant_stt_test", "6/minute", 6, 60),
            ("assistant_transcribe", "20/minute", 20, 60),
            ("assistant_llm_generate_title", "20/minute", 20, 60),
            ("assistant_agent_token", "12/minute", 12, 60),
        ];
        assert_eq!(ALL_THROTTLES.len(), expected.len());
        for (spec, &(scope, rate, requests, window_secs)) in
            ALL_THROTTLES.iter().zip(expected.iter())
        {
            assert_eq!(spec.scope, scope);
            assert_eq!(parse_rate(rate), Some((requests, window_secs)));
            assert_eq!((spec.requests, spec.window_secs), (requests, window_secs));
        }
    }

    #[test]
    fn parse_rate_covers_drf_period_letters() {
        assert_eq!(parse_rate("5/s"), Some((5, 1)));
        assert_eq!(parse_rate("5/sec"), Some((5, 1)));
        assert_eq!(parse_rate("5/m"), Some((5, 60)));
        assert_eq!(parse_rate("5/min"), Some((5, 60)));
        assert_eq!(parse_rate("5/h"), Some((5, 3600)));
        assert_eq!(parse_rate("5/hour"), Some((5, 3600)));
        assert_eq!(parse_rate("5/d"), Some((5, 86_400)));
        assert_eq!(parse_rate("5/day"), Some((5, 86_400)));
        assert_eq!(parse_rate("bogus"), None);
        assert_eq!(parse_rate("5/fortnight"), None);
    }

    #[test]
    fn cache_key_format_matches_drf() {
        assert_eq!(
            cache_key("assistant_message", "42"),
            "throttle_assistant_message_42"
        );
        assert_eq!(
            cache_key("assistant_agent_token", "10.0.0.1"),
            "throttle_assistant_agent_token_10.0.0.1"
        );
    }

    /// The 30/hour brake: 30 posts in the window pass, the 31st denies.
    #[test]
    fn message_brake_trips_at_thirty_per_hour() {
        let now = 1_700_000_000.0;
        let history: Vec<f64> = (0..30).map(|i| now - f64::from(i) * 60.0).collect();
        assert!(!allow_request(
            &history,
            now,
            MESSAGE_THROTTLE.requests,
            MESSAGE_THROTTLE.window_secs
        ));
        assert!(allow_request(
            &history[1..],
            now,
            MESSAGE_THROTTLE.requests,
            MESSAGE_THROTTLE.window_secs
        ));
    }

    /// Trim boundary is `<=`: an entry exactly one window old has passed out
    /// of the window (`while history and history[-1] <= now - duration`).
    #[test]
    fn window_boundary_entry_is_trimmed() {
        let now = 1_700_000_000.0;
        let history = [now - 3600.0];
        assert!(allow_request(&history, now, 1, 3600));
        let history = [now - 3600.0 + 0.001];
        assert!(!allow_request(&history, now, 1, 3600));
    }

    #[test]
    fn wait_matches_drf_formula() {
        let now = 1_700_000_000.0;
        // Denied but drainable: 30 live entries against a quota of 30 leaves
        // `available = 30 - 30 + 1 = 1`, so `wait` is the time until the
        // oldest entry expires. `None` needs the history to *exceed* quota.
        let full: Vec<f64> = (0..30).map(|i| now - f64::from(i)).collect();
        assert_eq!(throttle_wait(&full, now, 30, 3600), Some(3571.0));
        let overfull: Vec<f64> = (0..31).map(|i| now - f64::from(i)).collect();
        assert_eq!(throttle_wait(&overfull, now, 30, 3600), None);
        let roomy = &full[1..];
        let wait = throttle_wait(roomy, now, 30, 3600).expect("room for one");
        let remaining = 3600.0 - (now - roomy[roomy.len() - 1]);
        assert!((wait - remaining / 2.0).abs() < 1e-9);
        assert_eq!(throttle_wait(&[], now, 30, 3600), Some(3600.0 / 31.0));
    }

    /// Per-endpoint mapping: POST-only messages, every classed endpoint wired,
    /// and the deliberate non-applications (message GET, agent profile).
    #[test]
    fn endpoint_mapping_matches_views() {
        assert_eq!(messages_spec_for_method("POST"), Some(MESSAGE_THROTTLE));
        assert_eq!(messages_spec_for_method("GET"), None);
        assert_eq!(
            spec_for(ThrottlePoint::PostMessages).scope,
            "assistant_message"
        );
        assert_eq!(
            spec_for(ThrottlePoint::LlmConfigTest).scope,
            "assistant_llm_test"
        );
        assert_eq!(
            spec_for(ThrottlePoint::GenerateTitle).scope,
            "assistant_llm_generate_title"
        );
        assert_eq!(
            spec_for(ThrottlePoint::SttConfigTest).scope,
            "assistant_stt_test"
        );
        assert_eq!(
            spec_for(ThrottlePoint::Transcribe).scope,
            "assistant_transcribe"
        );
        assert_eq!(
            spec_for(ThrottlePoint::AgentToken).scope,
            "assistant_agent_token"
        );
    }

    #[tokio::test]
    async fn throttled_body_is_byte_identical() {
        let response = Throttled.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok().map(str::to_owned));
        let bytes = to_bytes(response.into_body(), 1024)
            .await
            .expect("read body");
        let body = String::from_utf8(bytes.to_vec()).expect("utf-8");
        assert_eq!(body, RATE_LIMIT_BODY);
        assert_eq!(
            body,
            r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#
        );
        assert_eq!(content_type.as_deref(), Some("application/json"));
    }
}
