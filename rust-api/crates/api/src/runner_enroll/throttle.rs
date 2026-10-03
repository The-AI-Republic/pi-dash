//! D-13 anonymous throttle: the global `AnonRateThrottle`, 30/minute.
//!
//! Ports the throttle guard on the three `AllowAny` D-13 endpoints that
//! inherit `DEFAULT_THROTTLE_CLASSES` without overriding it —
//! `RunnerEnrollEndpoint` (`enrollment.py:258-268`, "tightly auth-throttled"
//! per `design.md` §9.1), `MachineTokenRedeemEndpoint`
//! (`enrollment.py:853-859`, anti-brute-force on leaked tickets), and
//! `HealthEndpoint` (`register.py:23-28`, no `throttle_classes` attribute at
//! all) — from `settings/common.py:92-94`
//! (`DEFAULT_THROTTLE_CLASSES=(AnonRateThrottle,)`,
//! `DEFAULT_THROTTLE_RATES["anon"]="30/minute"`) plus the DRF
//! `SimpleRateThrottle` / `AnonRateThrottle` semantics (djangorestframework
//! 3.15.2, the pinned version).
//!
//! Fixture ids D13-F4 (`drf_failure_shapes.Throttled_anon`) and D13-F7 (the
//! `+ AnonRateThrottle 30/min -> 429 throttle shape on excess` pins on
//! `POST_runners_enroll`, `POST_machine_tokens_redeem`, and `GET_health`).
//!
//! Shape of the port: DRF's sliding-window decision as pure functions over a
//! caller-held timestamp history, plus the 429 denial rendering. Deliberately
//! *not* a token bucket: DRF allows a burst of the full quota inside any
//! window (`len(history) < num_requests` after trimming entries `<= now -
//! duration`), which a bucket with trickle refill would not reproduce. The
//! cache itself (the default django-redis cache, keyed `throttle_anon_<ip>`)
//! and the wiring stay with the handler layer — this module ports the rate,
//! the key, and the 429 body, which is this issue's scope. (The sibling
//! `crate::v1_openapi::throttle` ports the same DRF base class for another
//! surface; the two are intentionally not shared — no cross-domain code
//! dependency.)
//!
//! DRF semantics reproduced:
//!
//! * Rate strings parse as `<num>/<period>` with the period's first letter
//!   selecting seconds (`s/m/h/d` -> 1/60/3600/86400; [`parse_rate`],
//!   `throttling.py:97-107`).
//! * Ident is the client IP: `X-Forwarded-For` with all whitespace stripped
//!   when present, else `REMOTE_ADDR` ([`get_ident`]; `throttling.py:23-40`,
//!   `NUM_PROXIES` unset so the DRF default `None` branch applies).
//! * Cache key `throttle_anon_<ident>` ([`cache_key`];
//!   `throttling.py:64,165-180`). The `:1:` prefix seen on observed redis
//!   keys is cache versioning, not throttle logic.
//! * Authenticated callers are *exempt*: `AnonRateThrottle.get_cache_key`
//!   returns `None` when `request.user.is_authenticated`
//!   (`throttling.py:173-180`), and `allow_request` returns `True`
//!   immediately for a `None` key. Session-authed D-13 endpoints inherit the
//!   default class too but never throttle through it — which is why only the
//!   three `AllowAny` endpoints above are effectively throttled.
//! * [`allow_request`]: trim history entries `<= now - duration`, allow iff
//!   fewer than `num_requests` remain, else deny. Success records `now` at
//!   the front and re-caches with timeout `duration` (`throttle_success`,
//!   `throttling.py:109-147`).
//! * Denial raises `Throttled(wait)` (`views.py:352-371`), whose constructor
//!   ceils the wait (`exceptions.py:237`); the exception handler sets
//!   `Retry-After: '%d' % wait`, omitting the header when `wait` is `None`
//!   (`views.py:71-101`). [`retry_after_secs`] mirrors the ceil plus the
//!   truthiness check.
//! * The project's `auth_exception_handler` rewrites the `Throttled` data to
//!   `{"error_code": 5900, "error_message": "RATE_LIMIT_EXCEEDED"}`
//!   (`authentication/adapter/exception.py:25-31`, code table
//!   `authentication/adapter/error.py:71`); the DRF default detail (with its
//!   `wait` seconds) never reaches the client. D-13 views render stock
//!   compact JSON (`DEFAULT_RENDERER_CLASSES=(JSONRenderer,)`).
//!
//! Wiring note for the handler layer: `check_throttles` runs in `initial()`
//! *after* authentication and permissions — an unauthenticated caller on an
//! `AllowAny` endpoint reaches the throttle with no user, hence the anon key.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// DRF `scope` of the ported class (`settings/common.py:92-94`).
pub const ANON_SCOPE: &str = "anon";

/// The pinned rate string (`settings/common.py:94`).
pub const ANON_RATE: &str = "30/minute";

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

/// The global anonymous brake: 30/minute (`settings/common.py:92-94`).
pub const ANON_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: ANON_SCOPE,
    requests: 30,
    window_secs: 60,
};

/// Mirror of `SimpleRateThrottle.parse_rate` (`throttling.py:97-107`):
/// `<num>/<period>` where the period's first letter gives seconds
/// (`s/m/h/d` -> 1/60/3600/86400). Returns `None` for malformed input
/// (Python raises there; the table below is total, so `None` only fires on
/// caller error).
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

/// Mirror of `BaseThrottle.get_ident` (`throttling.py:23-40`) with the
/// project's settings (`NUM_PROXIES` unset -> DRF default `None` branch):
/// `X-Forwarded-For` with all whitespace stripped when the header is
/// present, else the remote address. An empty header value counts as absent
/// (Python's `if xff`); a whitespace-only value strips to the empty ident.
pub fn get_ident(x_forwarded_for: Option<&str>, remote_addr: &str) -> String {
    match x_forwarded_for {
        Some(xff) if !xff.is_empty() => xff.split_whitespace().collect(),
        _ => remote_addr.to_owned(),
    }
}

/// DRF `cache_format` (`throttling.py:64`) for the anon scope:
/// `'throttle_anon_%(ident)s'`.
pub fn cache_key(ident: &str) -> String {
    format!("throttle_{scope}_{ident}", scope = ANON_SCOPE)
}

/// Mirror of `AnonRateThrottle.get_cache_key` (`throttling.py:173-180`):
/// `None` (exempt, never throttled) when the caller is authenticated, else
/// the anon cache key for the ident.
pub fn anon_cache_key(is_authenticated: bool, ident: &str) -> Option<String> {
    if is_authenticated {
        None
    } else {
        Some(cache_key(ident))
    }
}

/// Mirror of `SimpleRateThrottle.allow_request` as a pure predicate
/// (`throttling.py:109-132`): `history` is the cached timestamp list
/// (newest first, as `throttle_success` stores it), `now` is `timer()`
/// (`time.time`). Entries `<= now - duration` have passed out of the window;
/// the request is allowed iff fewer than `num_requests` remain. On success
/// the caller records `now` at the front and re-caches with timeout
/// `duration`.
pub fn allow_request(history: &[f64], now: f64, num_requests: u32, window_secs: u64) -> bool {
    let duration = window_secs as f64;
    let live = history.iter().filter(|&&t| t > now - duration).count();
    live < num_requests as usize
}

/// Mirror of `SimpleRateThrottle.wait` (`throttling.py:149-163`):
/// recommended seconds before the next request, over the *trimmed* history
/// (`allow_request` sets `self.history` before `check_throttles` reads
/// `wait()`). `None` when no request is available (history at or over
/// quota).
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

/// Mirror of the `Retry-After` rendering chain: `Throttled.__init__` ceils
/// the wait (`exceptions.py:237`), then the exception handler emits
/// `Retry-After: '%d' % wait` only when the ceiled value is truthy
/// (`views.py:90-91`). `None` in (overfull history) means the header is
/// absent.
pub fn retry_after_secs(wait: Option<f64>) -> Option<i64> {
    let secs = wait?.ceil() as i64;
    if secs == 0 {
        None
    } else {
        Some(secs)
    }
}

/// `RATE_LIMIT_EXCEEDED` in `AUTHENTICATION_ERROR_CODES`
/// (`authentication/adapter/error.py:71`): the rewritten denial payload is
/// `{"error_code": 5900, "error_message": "RATE_LIMIT_EXCEEDED"}`
/// (`authentication/adapter/exception.py:25-31`).
pub const RATE_LIMIT_CODE: u16 = 5900;
/// See [`RATE_LIMIT_CODE`].
pub const RATE_LIMIT_MESSAGE: &str = "RATE_LIMIT_EXCEEDED";

/// Exact bytes of the 429 on the D-13 throttled endpoints: the rewritten
/// dict through the stock compact `JSONRenderer` (F4 `Throttled_anon`).
pub const DENIAL_BODY_JSON: &str = r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#;

/// Content-Type of the JSON denial: stock `JSONRenderer.media_type`.
pub const DENIAL_CONTENT_TYPE_JSON: &str = "application/json";

/// Rejection for a throttled enroll/redeem/health request: answers 429 with
/// the denial bytes plus `Retry-After` (absent when `wait` is `None`).
#[derive(Debug, Clone, Copy)]
pub struct AnonThrottled {
    /// The [`throttle_wait`] output driving `Retry-After`.
    pub wait: Option<f64>,
}

impl AnonThrottled {
    /// Build a denial with the given throttle wait.
    pub fn new(wait: Option<f64>) -> Self {
        AnonThrottled { wait }
    }
}

impl IntoResponse for AnonThrottled {
    fn into_response(self) -> Response {
        let mut builder = Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::CONTENT_TYPE, DENIAL_CONTENT_TYPE_JSON);
        if let Some(secs) = retry_after_secs(self.wait) {
            builder = builder.header(header::RETRY_AFTER, secs.to_string());
        }
        builder
            .body(axum::body::Body::from(DENIAL_BODY_JSON))
            .expect("static throttled response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// The anon rate from `settings/common.py:94` re-parses to the table
    /// numbers through the ported `parse_rate`.
    #[test]
    fn anon_rate_matches_python_settings() {
        assert_eq!(ANON_SCOPE, "anon");
        assert_eq!(ANON_RATE, "30/minute");
        assert_eq!(parse_rate(ANON_RATE), Some((30, 60)));
        assert_eq!(
            ANON_THROTTLE,
            ThrottleSpec {
                scope: "anon",
                requests: 30,
                window_secs: 60,
            }
        );
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

    /// `get_ident` vectors (`throttling.py:23-40`, `NUM_PROXIES` unset):
    /// X-Forwarded-For with all whitespace stripped wins, else remote addr.
    #[test]
    fn ident_prefers_stripped_xff() {
        assert_eq!(get_ident(None, "127.0.0.1"), "127.0.0.1");
        assert_eq!(get_ident(Some(""), "127.0.0.1"), "127.0.0.1");
        assert_eq!(get_ident(Some("1.2.3.4"), "127.0.0.1"), "1.2.3.4");
        assert_eq!(
            get_ident(Some("  1.2.3.4 , 5.6.7.8  "), "127.0.0.1"),
            "1.2.3.4,5.6.7.8"
        );
        assert_eq!(
            get_ident(Some("1.2.3.4,\t 5.6.7.8"), "127.0.0.1"),
            "1.2.3.4,5.6.7.8"
        );
        assert_eq!(get_ident(Some("   "), "127.0.0.1"), "");
    }

    /// Cache key format matches DRF (`throttle_anon_<ip>` — the suffix of
    /// the observed `:1:throttle_anon_127.0.0.1` redis key).
    #[test]
    fn cache_key_format_matches_drf() {
        assert_eq!(cache_key("127.0.0.1"), "throttle_anon_127.0.0.1");
        assert_eq!(
            cache_key("1.2.3.4,5.6.7.8"),
            "throttle_anon_1.2.3.4,5.6.7.8"
        );
    }

    /// `AnonRateThrottle.get_cache_key` (`throttling.py:173-180`):
    /// authenticated callers are exempt (key `None`), anonymous callers key
    /// on the ident. This is why only the three `AllowAny` D-13 endpoints
    /// are effectively throttled.
    #[test]
    fn authenticated_callers_are_exempt() {
        assert_eq!(anon_cache_key(true, "127.0.0.1"), None);
        assert_eq!(
            anon_cache_key(false, "127.0.0.1"),
            Some("throttle_anon_127.0.0.1".to_owned())
        );
    }

    /// Burst replay through `allow_request`/`throttle_success` ordering: 30
    /// allows, then denials from attempt 31 on.
    #[test]
    fn burst_allows_thirty_then_denies() {
        let now = 1_700_000_000.0;
        let mut history: Vec<f64> = Vec::new();
        let mut statuses = Vec::new();
        for _ in 0..33 {
            let allowed = allow_request(
                &history,
                now,
                ANON_THROTTLE.requests,
                ANON_THROTTLE.window_secs,
            );
            statuses.push(if allowed { 200 } else { 429 });
            if allowed {
                history.insert(0, now);
            }
        }
        let mut expected = vec![200; 30];
        expected.extend([429; 3]);
        assert_eq!(statuses, expected);
        assert_eq!(statuses.iter().position(|&s| s == 429), Some(30));
    }

    /// A window-old history admits again, and the quota edge (29 live
    /// allows, 30 live denies) holds.
    #[test]
    fn window_reset_reallows() {
        let now = 1_700_000_000.0;
        let aged: Vec<f64> = (0..30).map(|i| now - 61.0 - f64::from(i)).collect();
        assert!(allow_request(
            &aged,
            now,
            ANON_THROTTLE.requests,
            ANON_THROTTLE.window_secs
        ));
        let live29: Vec<f64> = (0..29).map(|i| now - f64::from(i)).collect();
        assert!(allow_request(
            &live29,
            now,
            ANON_THROTTLE.requests,
            ANON_THROTTLE.window_secs
        ));
        let live30: Vec<f64> = (0..30).map(|i| now - f64::from(i)).collect();
        assert!(!allow_request(
            &live30,
            now,
            ANON_THROTTLE.requests,
            ANON_THROTTLE.window_secs
        ));
    }

    /// Trim boundary is `<=`: an entry exactly one window old has passed out
    /// of the window (`while history and history[-1] <= now - duration`).
    #[test]
    fn window_boundary_entry_is_trimmed() {
        let now = 1_700_000_000.0;
        let history = [now - 60.0];
        assert!(allow_request(&history, now, 1, 60));
        let history = [now - 60.0 + 0.001];
        assert!(!allow_request(&history, now, 1, 60));
    }

    /// `wait()` matches the DRF formula (`throttling.py:149-163`).
    #[test]
    fn wait_matches_drf_formula() {
        let now = 1_700_000_000.0;
        let full: Vec<f64> = (0..30).map(|_| now).collect();
        assert_eq!(throttle_wait(&full, now, 30, 60), Some(60.0));
        let staggered: Vec<f64> = (0..30).map(|i| now - f64::from(i)).collect();
        assert_eq!(throttle_wait(&staggered, now, 30, 60), Some(31.0));
        let overfull: Vec<f64> = (0..31).map(|i| now - f64::from(i)).collect();
        assert_eq!(throttle_wait(&overfull, now, 30, 60), None);
        assert_eq!(throttle_wait(&[], now, 30, 60), Some(60.0 / 31.0));
    }

    /// `Retry-After` is the ceiled wait (`exceptions.py:237`,
    /// `views.py:90-91`), absent when the wait is `None`.
    #[test]
    fn retry_after_is_ceiled_wait() {
        assert_eq!(retry_after_secs(Some(58.8)), Some(59));
        assert_eq!(retry_after_secs(Some(60.0)), Some(60));
        assert_eq!(retry_after_secs(Some(0.4)), Some(1));
        assert_eq!(retry_after_secs(None), None);
        assert_eq!(retry_after_secs(Some(0.0)), None);
    }

    /// The 429 answers the exact F4 bytes with `application/json` plus the
    /// ceiled `Retry-After`; an overfull history omits the header.
    #[tokio::test]
    async fn denial_renders_bytes_and_retry_after() {
        let response = AnonThrottled::new(Some(58.8)).into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "59");
        let (_, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        assert_eq!(
            String::from_utf8(bytes.to_vec()).expect("utf8"),
            r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#
        );

        let response = AnonThrottled::new(None).into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().get(header::RETRY_AFTER).is_none());
    }
}
