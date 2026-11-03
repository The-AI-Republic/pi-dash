//! D-16 guard kernel: per-endpoint permissions + DRF throttle counters.
//!
//! Ports `apps/api/pi_dash/authentication/rate_limit.py` (all 47 lines),
//! the `permission_classes` / `throttle_classes` declarations on every D-16
//! view (`views/common.py:28`, `views/app|space/check.py:29-32`,
//! `views/app/magic.py:33-36`, `views/space/magic.py:31-32`,
//! `views/app|space/password_management.py:45-48`), and the DRF defaults
//! they fall back to (`settings/common.py:90-106`: `DEFAULT_THROTTLE_CLASSES`
//! `AnonRateThrottle`, `anon` 30/min, `DEFAULT_PERMISSION_CLASSES`
//! `IsAuthenticated`; `EXCEPTION_HANDLER`
//! `auth_exception_handler` in `adapter/exception.py:17-34`).
//!
//! The counter math mirrors DRF 3.18.1 `SimpleRateThrottle` (`parse_rate`,
//! `allow_request`, `throttle_success`, `wait`) with `f64` unix timestamps,
//! exactly like `time.time()`. Cache I/O stays the caller's job: these
//! functions take the cached history slice and return the history to store,
//! so the timeout (`duration`, per `cache.set(key, history, duration)`) and
//! the backend are deployment choices handlers make.
//!
//! Vectors replay `rust-api/fixtures/auth_session/FX-AUTH-05.guards.json`
//! (recorded by PIDASHCONV-279).
//!
//! Ported quirks (translated, not fixed; also listed in the PR):
//! - `MagicGenerateSpaceEndpoint` (`space/magic.py:31-32`) declares
//!   `permission_classes = [AllowAny]` but no `throttle_classes`, so it
//!   falls back to the settings default `AnonRateThrottle` (scope `anon`,
//!   same 30/min, separate counter namespace) while the app twin carries
//!   `AuthenticationThrottle` (scope `authentication`).
//! - `ChangePasswordEndpoint` / `SetUserPasswordEndpoint` (`common.py:47,99`)
//!   declare no `permission_classes`, so the settings default
//!   `IsAuthenticated` applies: anonymous callers get the 401
//!   `NotAuthenticated` body, never the view logic.
//! - `throttle_failure_view` (`rate_limit.py:21-28,40-47`) is dead code
//!   (zero callers); the live 429 path is the DRF default body rewritten
//!   by `auth_exception_handler`. Both bodies are pinned here and are
//!   identical.
//! - `EmailVerificationThrottle` (3/hour per user) has no D-16 view wiring;
//!   its spec and counter are ported, the endpoint table marks every D-16
//!   endpoint without it.

use serde_json::Value;

use super::shapes::{error_dict_json, not_authenticated_body, throttle_error_pairs};

// ---------------------------------------------------------------------------
// Throttle specs
// ---------------------------------------------------------------------------

/// `AuthenticationThrottle.rate` (`rate_limit.py:18`).
pub const AUTHENTICATION_THROTTLE_RATE: &str = "30/minute";
/// `AuthenticationThrottle.scope` (`rate_limit.py:19`).
pub const AUTHENTICATION_THROTTLE_SCOPE: &str = "authentication";
/// `EmailVerificationThrottle.rate` (`rate_limit.py:36`).
pub const EMAIL_VERIFICATION_THROTTLE_RATE: &str = "3/hour";
/// `EmailVerificationThrottle.scope` (`rate_limit.py:37`).
pub const EMAIL_VERIFICATION_THROTTLE_SCOPE: &str = "email_verification";
/// Settings default scope throttling the views that declare no
/// `throttle_classes` (`common.py:92-93`: `DEFAULT_THROTTLE_CLASSES`
/// `AnonRateThrottle`, `DEFAULT_THROTTLE_RATES["anon"]`).
pub const DEFAULT_ANON_SCOPE: &str = "anon";
/// Settings default anon rate (`common.py:94`).
pub const DEFAULT_ANON_RATE: &str = "30/minute";

/// Parse a DRF rate string (`throttling.py:97-107`): `None` stays `None`
/// (unthrottled, like `rate = None`); otherwise `(requests, seconds)`.
/// Only the period's first character decides (`period[0]`), so `"minute"`
/// and `"m"` both mean 60. An unknown period is `None` (DRF raises
/// `KeyError`; callers treat `None` as unconfigured, same as `rate=None`).
pub fn parse_rate(rate: Option<&str>) -> Option<(u32, u64)> {
    let rate = rate?;
    let (num, period) = rate.split_once('/')?;
    let num_requests: u32 = num.parse().ok()?;
    let duration = match period.as_bytes().first()? {
        b's' => 1,
        b'm' => 60,
        b'h' => 3600,
        b'd' => 86400,
        _ => return None,
    };
    Some((num_requests, duration))
}

/// Cache key (`throttling.py:64`):
/// `cache_format = 'throttle_%(scope)s_%(ident)s'`.
pub fn throttle_cache_key(scope: &str, ident: &str) -> String {
    format!("throttle_{scope}_{ident}")
}

/// `AnonRateThrottle.get_cache_key` (`throttling.py:173-180`): `None` for
/// authenticated callers (never throttled), otherwise the scoped IP key.
pub fn anon_cache_key(scope: &str, authenticated: bool, ident: &str) -> Option<String> {
    if authenticated {
        return None;
    }
    Some(throttle_cache_key(scope, ident))
}

/// `UserRateThrottle.get_cache_key` (`throttling.py:193-202`): the user pk
/// when authenticated, otherwise the IP key.
pub fn user_cache_key(scope: &str, user_pk: Option<&str>, ident: &str) -> String {
    match user_pk {
        Some(pk) => throttle_cache_key(scope, pk),
        None => throttle_cache_key(scope, ident),
    }
}

/// Outcome of one `allow_request` pass: whether the request goes through
/// and the history to cache (DRF caches with timeout `duration` on success
/// and leaves the trimmed history unrecorded on denial).
#[derive(Debug, Clone, PartialEq)]
pub struct ThrottleDecision {
    pub allowed: bool,
    /// Newest timestamp first, like DRF's `history.insert(0, now)`.
    pub history: Vec<f64>,
}

/// `allow_request` (`throttling.py:109-132`) minus cache I/O: drop expired
/// entries from the tail (`history[-1] <= now - duration`), deny when
/// `len(history) >= num_requests` (recording nothing), else allow and
/// prepend `now`.
pub fn allow_request(
    history: &[f64],
    num_requests: u32,
    duration_secs: u64,
    now: f64,
) -> ThrottleDecision {
    let cutoff = now - duration_secs as f64;
    let mut trimmed: Vec<f64> = history.to_vec();
    while matches!(trimmed.last(), Some(oldest) if *oldest <= cutoff) {
        trimmed.pop();
    }
    if trimmed.len() as u32 >= num_requests {
        return ThrottleDecision {
            allowed: false,
            history: trimmed,
        };
    }
    trimmed.insert(0, now);
    ThrottleDecision {
        allowed: true,
        history: trimmed,
    }
}

/// `wait` (`throttling.py:149-163`): recommended seconds before retrying,
/// over the trimmed (denied, unrecorded) history. `None` when no request
/// could free a slot.
pub fn throttle_wait(
    trimmed_history: &[f64],
    num_requests: u32,
    duration_secs: u64,
    now: f64,
) -> Option<f64> {
    let duration = duration_secs as f64;
    let remaining = match trimmed_history.last() {
        Some(oldest) => duration - (now - oldest),
        None => duration,
    };
    let available = num_requests as i64 - trimmed_history.len() as i64 + 1;
    if available <= 0 {
        return None;
    }
    Some(remaining / available as f64)
}

// ---------------------------------------------------------------------------
// Denial bodies
// ---------------------------------------------------------------------------

/// Live 429 body: `auth_exception_handler` rewrites DRF's default
/// throttled response to the 5900 error dict (`exception.py:26-31`;
/// identical to the dead `throttle_failure_view` body).
pub fn throttle_denied_body() -> Value {
    let pairs = throttle_error_pairs();
    serde_json::from_str(&error_dict_json(&pairs)).expect("throttle pairs serialize")
}

/// Byte-exact live 429 body string.
pub fn throttle_denied_json() -> String {
    error_dict_json(&throttle_error_pairs())
}

/// DRF's default throttled body before `auth_exception_handler` rewrites
/// it (`{"detail": "Request was throttled."}`, status 429).
pub fn drf_throttled_body() -> Value {
    serde_json::json!({"detail": "Request was throttled."})
}

/// 401 body for the `IsAuthenticated` endpoints: DRF `NotAuthenticated`
/// default, passed through untouched (`exception.py:22-24`).
pub fn unauthenticated_body() -> Value {
    not_authenticated_body()
}

// ---------------------------------------------------------------------------
// Per-endpoint table
// ---------------------------------------------------------------------------

/// App vs space surface: every form-POST view exists twice, the DRF
/// endpoints `check` / `magic-generate` / `forgot-password` too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    App,
    Space,
}

/// Every D-16 endpoint this issue guards (OAuth `google|github|gitlab|gitea`
/// views are D-17 and out of scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthEndpoint {
    EmailCheck(Surface),
    MagicGenerate(Surface),
    ForgotPassword(Surface),
    CsrfToken,
    ChangePassword,
    SetPassword,
    SignIn(Surface),
    SignUp(Surface),
    MagicSignIn(Surface),
    MagicSignUp(Surface),
    ResetPassword(Surface),
    SignOut(Surface),
}

/// DRF permission layer per endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionPolicy {
    /// Explicit `permission_classes = [AllowAny]`.
    AllowAny,
    /// No declaration: settings `DEFAULT_PERMISSION_CLASSES`
    /// (`IsAuthenticated`, `common.py:116`).
    IsAuthenticated,
    /// Plain `django.views.View`: no DRF permission layer at all; the view
    /// reads the session/user itself (form-POST views).
    SessionDependent,
}

/// DRF throttle layer per endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottlePolicy {
    /// No DRF throttle layer (plain `django.views.View`).
    None,
    /// Explicit `throttle_classes = [AuthenticationThrottle]`.
    Authentication,
    /// No declaration: settings `DEFAULT_THROTTLE_CLASSES`
    /// (`AnonRateThrottle`, scope `anon`, 30/min).
    DefaultAnon,
}

/// The guards on one endpoint: permission layer + throttle layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointGuards {
    pub permission: PermissionPolicy,
    pub throttle: ThrottlePolicy,
}

/// Per-endpoint guard table, read straight off the class declarations
/// (see the `grep` map in the module docs). The one asymmetry is
/// `MagicGenerate(Space)`: AllowAny like its app twin, but default-Anon
/// throttling instead of `AuthenticationThrottle`.
pub fn endpoint_guards(endpoint: AuthEndpoint) -> EndpointGuards {
    use AuthEndpoint::*;
    use PermissionPolicy::*;
    use ThrottlePolicy::*;
    match endpoint {
        EmailCheck(_) => EndpointGuards {
            permission: AllowAny,
            throttle: Authentication,
        },
        MagicGenerate(Surface::App) => EndpointGuards {
            permission: AllowAny,
            throttle: Authentication,
        },
        // `space/magic.py:31-32`: no `throttle_classes` -> default Anon.
        MagicGenerate(Surface::Space) => EndpointGuards {
            permission: AllowAny,
            throttle: DefaultAnon,
        },
        ForgotPassword(_) => EndpointGuards {
            permission: AllowAny,
            throttle: Authentication,
        },
        // `common.py:28-29`: AllowAny, no `throttle_classes` -> default Anon.
        CsrfToken => EndpointGuards {
            permission: AllowAny,
            throttle: DefaultAnon,
        },
        // `common.py:47,99`: no `permission_classes` -> IsAuthenticated;
        // no `throttle_classes` -> default Anon.
        ChangePassword | SetPassword => EndpointGuards {
            permission: IsAuthenticated,
            throttle: DefaultAnon,
        },
        SignIn(_) | SignUp(_) | MagicSignIn(_) | MagicSignUp(_) | ResetPassword(_) | SignOut(_) => {
            EndpointGuards {
                permission: SessionDependent,
                throttle: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_session/FX-AUTH-05.guards.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn throttle_specs_match_fixture() {
        let fx = fixture();
        let throttles = fx.get("throttles").expect("throttles");
        let auth = &throttles["AuthenticationThrottle"];
        assert_eq!(auth["rate"], Value::from(AUTHENTICATION_THROTTLE_RATE));
        assert_eq!(auth["scope"], Value::from(AUTHENTICATION_THROTTLE_SCOPE));
        assert_eq!(
            parse_rate(Some(AUTHENTICATION_THROTTLE_RATE)),
            Some((30, 60))
        );
        let email = &throttles["EmailVerificationThrottle"];
        assert_eq!(email["rate"], Value::from(EMAIL_VERIFICATION_THROTTLE_RATE));
        assert_eq!(
            email["scope"],
            Value::from(EMAIL_VERIFICATION_THROTTLE_SCOPE)
        );
        assert_eq!(
            parse_rate(Some(EMAIL_VERIFICATION_THROTTLE_RATE)),
            Some((3, 3600))
        );
        // Base classes from the fixture: Anon vs User flavors.
        assert_eq!(
            auth["base"],
            serde_json::json!(["AnonRateThrottle", "SimpleRateThrottle"])
        );
        assert_eq!(
            email["base"],
            serde_json::json!(["UserRateThrottle", "SimpleRateThrottle"])
        );
        // Settings default behind the DefaultAnon policy.
        assert_eq!(parse_rate(Some(DEFAULT_ANON_RATE)), Some((30, 60)));
        assert_eq!(DEFAULT_ANON_SCOPE, "anon");
    }

    #[test]
    fn throttle_429_bodies_match_fixture() {
        let fx = fixture();
        let bodies = fx.get("throttle_429_bodies").expect("bodies");
        let want = "{\"error_code\":5900,\"error_message\":\"RATE_LIMIT_EXCEEDED\"}";
        assert_eq!(throttle_denied_json(), want);
        assert_eq!(bodies["AuthenticationThrottle"]["status"], 429);
        assert_eq!(bodies["AuthenticationThrottle"]["body"].to_string(), want);
        assert_eq!(bodies["EmailVerificationThrottle"]["status"], 429);
        assert_eq!(
            bodies["EmailVerificationThrottle"]["body"].to_string(),
            want
        );
        assert_eq!(throttle_denied_body().to_string(), want);
        // The production DRF path starts from the default throttled body
        // and is rewritten to the same 5900 dict.
        assert_eq!(
            bodies["production_drf_path"],
            serde_json::json!({"status": 429, "body": {"detail": "Request was throttled."}})
        );
        assert_eq!(
            drf_throttled_body(),
            serde_json::json!({"detail": "Request was throttled."})
        );
    }

    #[test]
    fn production_401_matches_fixture() {
        let fx = fixture();
        assert_eq!(
            fx["permissions"]["production_401"],
            serde_json::json!({
                "status": 401,
                "body": {"detail": "Authentication credentials were not provided."}
            })
        );
        assert_eq!(
            unauthenticated_body(),
            serde_json::json!({"detail": "Authentication credentials were not provided."})
        );
    }

    #[test]
    fn allow_any_endpoints_match_fixture() {
        use AuthEndpoint::*;
        // Fixture AllowAny_explicit groups.
        for endpoint in [
            EmailCheck(Surface::App),
            EmailCheck(Surface::Space),
            MagicGenerate(Surface::App),
            MagicGenerate(Surface::Space),
            ForgotPassword(Surface::App),
            ForgotPassword(Surface::Space),
            CsrfToken,
        ] {
            assert_eq!(
                endpoint_guards(endpoint).permission,
                PermissionPolicy::AllowAny,
                "{endpoint:?}"
            );
        }
        // Fixture `declared` map names the same four slugs.
        let fx = fixture();
        let declared = fx["permissions"]["declared"].as_object().expect("declared");
        assert_eq!(declared.len(), 6);
        for slug in ["EmailCheck", "MagicGenerate", "ForgotPassword", "CSRFToken"] {
            assert_eq!(declared[slug], serde_json::json!(["AllowAny"]), "{slug}");
        }
    }

    #[test]
    fn password_endpoints_require_authentication() {
        // No `permission_classes` on either class (common.py:47,99), so the
        // settings default IsAuthenticated applies; the merged contract
        // tests pin anonymous -> 401 with a `detail` body.
        for endpoint in [AuthEndpoint::ChangePassword, AuthEndpoint::SetPassword] {
            let guards = endpoint_guards(endpoint);
            assert_eq!(guards.permission, PermissionPolicy::IsAuthenticated);
            assert_eq!(guards.throttle, ThrottlePolicy::DefaultAnon);
        }
    }

    #[test]
    fn plain_views_are_session_dependent() {
        use AuthEndpoint::*;
        for endpoint in [
            SignIn(Surface::App),
            SignIn(Surface::Space),
            SignUp(Surface::App),
            SignUp(Surface::Space),
            MagicSignIn(Surface::App),
            MagicSignIn(Surface::Space),
            MagicSignUp(Surface::App),
            MagicSignUp(Surface::Space),
            ResetPassword(Surface::App),
            ResetPassword(Surface::Space),
            SignOut(Surface::App),
            SignOut(Surface::Space),
        ] {
            assert_eq!(
                endpoint_guards(endpoint),
                EndpointGuards {
                    permission: PermissionPolicy::SessionDependent,
                    throttle: ThrottlePolicy::None,
                },
                "{endpoint:?}"
            );
        }
    }

    #[test]
    fn space_magic_generate_uses_default_anon_throttle() {
        // The single asymmetry in the table: space/magic.py:31-32 declares
        // no `throttle_classes`.
        assert_eq!(
            endpoint_guards(AuthEndpoint::MagicGenerate(Surface::Space)).throttle,
            ThrottlePolicy::DefaultAnon
        );
        assert_eq!(
            endpoint_guards(AuthEndpoint::MagicGenerate(Surface::App)).throttle,
            ThrottlePolicy::Authentication
        );
    }

    #[test]
    fn anon_drive_allows_30_then_denies() {
        // Fixture throttle_drive: anon_first30_all_true + anon_req31 false.
        let (num, duration) = parse_rate(Some(AUTHENTICATION_THROTTLE_RATE)).expect("rate");
        let mut history: Vec<f64> = vec![];
        for i in 0..30 {
            let decision = allow_request(&history, num, duration, 1_000_000.0 + i as f64);
            assert!(decision.allowed, "request {i} allowed");
            history = decision.history;
        }
        assert_eq!(history.len(), 30);
        let denied = allow_request(&history, num, duration, 1_000_030.0);
        assert!(!denied.allowed);
        // A denial records nothing: the trimmed history is unchanged.
        assert_eq!(denied.history, history);
    }

    #[test]
    fn authed_callers_skip_the_anon_throttle() {
        // Fixture authed_5_all_true: AnonRateThrottle.get_cache_key is None
        // for authenticated users, so allow_request is never reached.
        assert_eq!(
            anon_cache_key(AUTHENTICATION_THROTTLE_SCOPE, true, "1.2.3.4"),
            None
        );
        assert_eq!(
            anon_cache_key(AUTHENTICATION_THROTTLE_SCOPE, false, "1.2.3.4"),
            Some("throttle_authentication_1.2.3.4".to_string())
        );
    }

    #[test]
    fn email_verification_allows_3_per_hour_per_user() {
        let (num, duration) = parse_rate(Some(EMAIL_VERIFICATION_THROTTLE_RATE)).expect("rate");
        assert_eq!((num, duration), (3, 3600));
        // Per-user key (UserRateThrottle): the pk when authenticated.
        assert_eq!(
            user_cache_key(EMAIL_VERIFICATION_THROTTLE_SCOPE, Some("42"), "1.2.3.4"),
            "throttle_email_verification_42"
        );
        assert_eq!(
            user_cache_key(EMAIL_VERIFICATION_THROTTLE_SCOPE, None, "1.2.3.4"),
            "throttle_email_verification_1.2.3.4"
        );
        let mut history: Vec<f64> = vec![];
        for i in 0..3 {
            let decision = allow_request(&history, num, duration, 2_000_000.0 + i as f64);
            assert!(decision.allowed, "request {i} allowed");
            history = decision.history;
        }
        assert!(!allow_request(&history, num, duration, 2_000_003.0).allowed);
        // An hour later the window slides past and the endpoint is usable.
        assert!(allow_request(&history, num, duration, 2_003_601.0).allowed);
    }

    #[test]
    fn wait_matches_drf_arithmetic() {
        // history newest-first [10, 5, 0], num 3, duration 60, now 20:
        // remaining = 60 - (20 - 0) = 40, available = 3 - 3 + 1 = 1.
        assert_eq!(throttle_wait(&[10.0, 5.0, 0.0], 3, 60, 20.0), Some(40.0));
        // Empty history: full duration over (num + 1) slots... DRF gives
        // duration / (num + 1): available = num - 0 + 1.
        assert_eq!(throttle_wait(&[], 30, 60, 0.0), Some(60.0 / 31.0));
        // Saturated history: None.
        let full: Vec<f64> = (0..31).map(|i| 100.0 - i as f64).collect();
        assert_eq!(throttle_wait(&full, 30, 60, 100.0), None);
    }

    #[test]
    fn parse_rate_vectors() {
        assert_eq!(parse_rate(None), None);
        assert_eq!(parse_rate(Some("30/minute")), Some((30, 60)));
        assert_eq!(parse_rate(Some("3/hour")), Some((3, 3600)));
        // Only period[0] decides, like DRF.
        assert_eq!(parse_rate(Some("5/m")), Some((5, 60)));
        assert_eq!(parse_rate(Some("1/d")), Some((1, 86400)));
        assert_eq!(parse_rate(Some("10/s")), Some((10, 1)));
        // Unknown period: DRF raises KeyError; the port returns None
        // (treated as unconfigured, like rate=None).
        assert!(parse_rate(Some("bogus")).is_none());
        assert_eq!(parse_rate(Some("10/fortnight")), None);
    }
}
