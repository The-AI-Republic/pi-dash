//! D-17 authentication guard kernel: device-flow constants, the
//! `device/start/` throttle spec, the verification-URI builder, and the
//! per-endpoint auth/permission matrix.
//!
//! Ports `apps/api/pi_dash/authentication/views/cli/device.py:54-66`
//! (flow constants), `:132-143` (`DeviceCodeStartThrottle` +
//! `_verification_uri`), and the `permission_classes` /
//! `authentication_classes` / `throttle_classes` declarations on all six
//! device endpoints (`:145-511`, routes in
//! `apps/api/pi_dash/api/urls/auth.py:1-47`), plus the auth shape of the
//! sixteen OAuth views (`authentication/views/app+space/`
//! `{google,github,gitlab,gitea}.py`: plain `django.views.View` `GET`
//! handlers with no DRF gates; `host` / `state` / `next_path` session keys).
//! The throttle rate string lives in `settings/common.py:93-101`
//! (`DEFAULT_THROTTLE_RATES["auth_device_start"]`).
//!
//! Vectors replay `rust-api/fixtures/auth_oauth/F9_guards.golden.json`
//! (recorded by PIDASHCONV-324, AUTHOAUTH-F9).
//!
//! Out of scope (sibling issues): the endpoint bodies (PIDASHCONV-342/343,
//! AUTHOAUTH-F12), the OAuth initiate/callback redirects (PIDASHCONV-335/336/
//! 339/341, AUTHOAUTH-F10/F11), account upsert + device helpers
//! (PIDASHCONV-327, AUTHOAUTH-F6/F7/F8).
//!
//! Error-dict rendering (`adapter/error.py:77-92`, `get_error_dict`) is
//! already ported as `pidash_services::auth_oauth::error` (PIDASHCONV-325,
//! AUTHOAUTH-F5) and is referenced here, not redefined.
//!
//! # Throttle semantics
//!
//! `DeviceCodeStartThrottle` subclasses DRF's `AnonRateThrottle`
//! (`device.py:132-138`): per-IP, and — like every `AnonRateThrottle` —
//! only unauthenticated callers are counted. The counter math itself is
//! DRF's `SimpleRateThrottle`, already ported as
//! `pidash_services::auth_session::guards` (D-16); handlers drive it with
//! [`DEVICE_START_THROTTLE_SCOPE`], [`DEVICE_START_THROTTLE_RATE_STR`],
//! and [`device_start_cache_key`]. Nothing here duplicates that kernel.

/// Grant window for a device/user code pair (`device.py:57`).
///
/// `DEVICE_CODE_TTL = timedelta(minutes=10)`: `start` stamps
/// `expires_at = now + TTL` and answers `expires_in = int(600)`.
pub const DEVICE_CODE_TTL_SECS: u64 = 600;
/// Recommended CLI polling interval (`device.py:58`, RFC 8628 §3.2).
///
/// Answered as the `interval` field of the `start` response.
pub const DEVICE_CODE_POLL_INTERVAL_SECS: u64 = 5;
/// Floor between polls (`device.py:59`).
///
/// `slow_down` rejections do NOT bump `last_polled_at` (anti-starvation);
/// the floor doubles on each violation up to 30s (handler logic, F12).
pub const DEVICE_CODE_MIN_POLL_GAP_SECS: u64 = 3;
/// Bounded `start` create retries on unique-constraint collision
/// (`device.py:66`); exhaustion answers 503 `internal_error`.
pub const DEVICE_CODE_START_MAX_RETRIES: u32 = 5;

/// `DeviceCodeStartThrottle.scope` (`device.py:138`).
pub const DEVICE_START_THROTTLE_SCOPE: &str = "auth_device_start";
/// `DEFAULT_THROTTLE_RATES["auth_device_start"]`
/// (`settings/common.py:101`): per-IP cap, one call per `pidash auth login`.
pub const DEVICE_START_THROTTLE_RATE_STR: &str = "20/minute";

/// Parse [`DEVICE_START_THROTTLE_RATE_STR`] into `(requests, seconds)`
/// via the shared DRF `parse_rate` kernel: `(20, 60)`.
pub fn device_start_throttle_rate() -> Option<(u32, u64)> {
    pidash_services::auth_session::guards::parse_rate(Some(DEVICE_START_THROTTLE_RATE_STR))
}

/// `AnonRateThrottle` cache key for `device/start/`
/// (`throttle_auth_device_start_<ip>`); `None` for authenticated callers,
/// who are never throttled — mirrors `device.py:132-138` over DRF
/// `throttling.py:173-180`.
pub fn device_start_cache_key(authenticated: bool, ident: &str) -> Option<String> {
    pidash_services::auth_session::guards::anon_cache_key(
        DEVICE_START_THROTTLE_SCOPE,
        authenticated,
        ident,
    )
}

/// `_verification_uri` (`device.py:141-143`):
/// `f"{base_host(request=request).rstrip('/')}/auth/device/"`.
///
/// `base_host` keeps a trailing slash in the golden fixture, so the
/// `rstrip('/')` matters: without it the URI would carry a double slash.
pub fn verification_uri(base_host: &str) -> String {
    format!(
        "{}/auth/device/",
        base_host.strip_suffix('/').unwrap_or(base_host)
    )
}

// ---------------------------------------------------------------------------
// Device endpoint auth/permission matrix
// ---------------------------------------------------------------------------

/// One row of the device-endpoint matrix: the DRF gate triple plus the
/// route identity (`device.py:145-511`, `api/urls/auth.py:1-47`).
///
/// `permission` / `authentication` use the Python class names verbatim;
/// `authentication` is empty where the view sets
/// `authentication_classes: list = []` (fully anonymous — no session, no
/// token — rather than the settings default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceEndpointGuard {
    /// URL name in `api/urls/auth.py` (e.g. `"auth-device-start"`).
    pub name: &'static str,
    /// Full path under `/api/v1/` (e.g. `"auth/device/start/"`).
    pub path: &'static str,
    /// HTTP method the view implements.
    pub method: &'static str,
    /// `permission_classes` entry, e.g. `"AllowAny"`.
    pub permission: &'static str,
    /// `authentication_classes` entries; empty means `[]`.
    pub authentication: &'static [&'static str],
    /// `throttle_classes` entry, if any.
    pub throttle: Option<&'static str>,
}

/// Per-endpoint gates for the six device routes, in `device.py` class
/// order (`:154-155,198-199,292-293,392-393,414-415,493-494`, the same
/// order the F9 fixture records).
///
/// Only `start` is throttled; only `approve` is session-authenticated
/// (logged-in human in a browser); `workspaces` / `machine-token` /
/// `revoke` take `APIKeyAuthentication` (CLI `X-Api-Key` token, either an
/// `APIToken` or an `mt_` machine token).
pub const DEVICE_ENDPOINT_GUARDS: [DeviceEndpointGuard; 6] = [
    DeviceEndpointGuard {
        name: "auth-device-start",
        path: "auth/device/start/",
        method: "POST",
        permission: "AllowAny",
        authentication: &[],
        throttle: Some("DeviceCodeStartThrottle"),
    },
    DeviceEndpointGuard {
        name: "auth-device-approve",
        path: "auth/device/approve/",
        method: "POST",
        permission: "IsAuthenticated",
        authentication: &["BaseSessionAuthentication"],
        throttle: None,
    },
    DeviceEndpointGuard {
        name: "auth-device-token",
        path: "auth/device/token/",
        method: "POST",
        permission: "AllowAny",
        authentication: &[],
        throttle: None,
    },
    DeviceEndpointGuard {
        name: "auth-workspaces",
        path: "auth/workspaces/",
        method: "GET",
        permission: "IsAuthenticated",
        authentication: &["APIKeyAuthentication"],
        throttle: None,
    },
    DeviceEndpointGuard {
        name: "auth-machine-token",
        path: "auth/machine-token/",
        method: "POST",
        permission: "IsAuthenticated",
        authentication: &["APIKeyAuthentication"],
        throttle: None,
    },
    DeviceEndpointGuard {
        name: "auth-revoke",
        path: "auth/revoke/",
        method: "POST",
        permission: "IsAuthenticated",
        authentication: &["APIKeyAuthentication"],
        throttle: None,
    },
];

/// Look up a device endpoint's gates by its URL name.
pub fn device_endpoint_guard(name: &str) -> Option<DeviceEndpointGuard> {
    DEVICE_ENDPOINT_GUARDS
        .iter()
        .find(|g| g.name == name)
        .copied()
}

// ---------------------------------------------------------------------------
// OAuth view auth shape
// ---------------------------------------------------------------------------

/// The sixteen OAuth views (`views/app+space/{google,github,gitlab,gitea}.py`,
/// routes in `authentication/urls.py:66-125`) are plain
/// `django.views.View` `GET` handlers: no `permission_classes`, no
/// `authentication_classes`, no `throttle_classes`. CSRF/session state
/// travels in the Django session, not in DRF gates.
pub const OAUTH_VIEW_BASE: &str = "django.views.View";
/// HTTP method every OAuth initiate/callback view implements.
pub const OAUTH_VIEW_METHOD: &str = "GET";

/// Session keys an app-surface initiate writes
/// (`views/app/{google,github,gitlab,gitea}.py:29-50`): `host`
/// (always), `next_path` (only when the query carries it), `state`.
pub const OAUTH_APP_INITIATE_SESSION_KEYS: [&str; 3] = ["host", "next_path", "state"];
/// Session keys the space gitea initiate writes
/// (`views/space/gitea.py:28-49`): same triple as app (with
/// `validate_next_path` on `next_path`).
pub const OAUTH_SPACE_GITEA_INITIATE_SESSION_KEYS: [&str; 3] = ["host", "next_path", "state"];
/// Session keys the space google/github/gitlab initiates write
/// (`views/space/google.py:27-46`, `github.py:28-46`, `gitlab.py:28-47`):
/// `host` + `state` only — they read `next_path` from the query for the
/// error redirect but never store it. Ported as-is.
pub const OAUTH_SPACE_INITIATE_SESSION_KEYS: [&str; 2] = ["host", "state"];
/// Session keys every callback reads (`views/app/*.py:64-67`,
/// `views/space/*.py:61-66`): `host`, `next_path`, `state`.
pub const OAUTH_CALLBACK_SESSION_KEYS: [&str; 3] = ["host", "next_path", "state"];

/// OAuth provider slug in `authentication/urls.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OauthProvider {
    Google,
    Github,
    Gitlab,
    Gitea,
}

impl OauthProvider {
    /// `*_NOT_CONFIGURED` error name for this provider
    /// (`adapter/error.py:43-47`), via the shared services kernel.
    pub fn not_configured_name(self) -> Option<&'static str> {
        pidash_services::auth_oauth::error::not_configured_name(match self {
            OauthProvider::Google => "google",
            OauthProvider::Github => "github",
            OauthProvider::Gitlab => "gitlab",
            OauthProvider::Gitea => "gitea",
        })
    }
}

/// Session keys an initiate view writes, by surface and provider
/// (translate-only: the space google/github/gitlab pair omits
/// `next_path`, every other initiate writes the triple).
pub fn initiate_session_keys(is_space: bool, provider: OauthProvider) -> &'static [&'static str] {
    if !is_space || provider == OauthProvider::Gitea {
        &OAUTH_APP_INITIATE_SESSION_KEYS
    } else {
        &OAUTH_SPACE_INITIATE_SESSION_KEYS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/auth_oauth")
    }

    fn f9() -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join("F9_guards.golden.json"))
            .expect("read F9_guards.golden.json");
        serde_json::from_str(&body).expect("F9_guards.golden.json is valid JSON")
    }

    #[test]
    fn flow_constants_match_f9() {
        let c = &f9()["constants"];
        // `timedelta(minutes=10)`; the start response renders it as int(600).
        assert_eq!(DEVICE_CODE_TTL_SECS, 600);
        assert!(c["DEVICE_CODE_TTL"]
            .as_str()
            .unwrap()
            .contains("minutes=10"));
        assert!(c["DEVICE_CODE_TTL"].as_str().unwrap().contains("int(600)"));
        assert_eq!(DEVICE_CODE_POLL_INTERVAL_SECS, 5);
        assert_eq!(
            c["DEVICE_CODE_POLL_INTERVAL_SECONDS"].as_u64().unwrap(),
            DEVICE_CODE_POLL_INTERVAL_SECS
        );
        // `timedelta(seconds=3)` floor; slow_down rejections skip the bump.
        assert_eq!(DEVICE_CODE_MIN_POLL_GAP_SECS, 3);
        assert!(c["DEVICE_CODE_MIN_POLL_GAP"]
            .as_str()
            .unwrap()
            .contains("seconds=3"));
        assert_eq!(DEVICE_CODE_START_MAX_RETRIES, 5);
        assert_eq!(
            c["DEVICE_CODE_START_MAX_RETRIES"].as_u64().unwrap(),
            u64::from(DEVICE_CODE_START_MAX_RETRIES)
        );
    }

    #[test]
    fn throttle_scope_and_rate_match_f9() {
        let t = &f9()["throttle"];
        assert_eq!(DEVICE_START_THROTTLE_SCOPE, "auth_device_start");
        assert_eq!(DEVICE_START_THROTTLE_SCOPE, t["scope"].as_str().unwrap());
        assert_eq!(DEVICE_START_THROTTLE_RATE_STR, "20/minute");
        assert!(t["rate"].as_str().unwrap().contains("20/minute"));
        assert!(t["rate"].as_str().unwrap().contains("per-IP"));
        assert_eq!(device_start_throttle_rate(), Some((20, 60)));
        // Anon semantics: authenticated callers bypass, anonymous callers
        // get the scoped per-IP key.
        assert_eq!(device_start_cache_key(true, "1.2.3.4"), None);
        assert_eq!(
            device_start_cache_key(false, "1.2.3.4").as_deref(),
            Some("throttle_auth_device_start_1.2.3.4")
        );
    }

    #[test]
    fn verification_uri_matches_golden() {
        let v = &f9()["verification_uri"];
        assert_eq!(
            v["code"].as_str().unwrap(),
            "f\"{base_host(request=request).rstrip('/')}/auth/device/\""
        );
        let golden = &v["golden_example"];
        assert_eq!(
            verification_uri(golden["base_host"].as_str().unwrap()),
            golden["out"].as_str().unwrap()
        );
        // The rstrip('/') matters: a bare host with no trailing slash and
        // one with it render identically, never with a double slash.
        assert_eq!(
            verification_uri("https://app.example.com"),
            "https://app.example.com/auth/device/"
        );
        assert_eq!(
            verification_uri("https://app.example.com/"),
            "https://app.example.com/auth/device/"
        );
    }

    #[test]
    fn device_matrix_has_one_row_per_endpoint() {
        let rows = f9()["endpoint_matrix"].as_array().unwrap().to_vec();
        // Row 0 is the OAuth aggregate row (asserted in
        // `oauth_view_shape_...`); rows 1..=6 are the device endpoints.
        assert_eq!(rows.len(), 7);
        assert_eq!(DEVICE_ENDPOINT_GUARDS.len(), 6);
        for (guard, row) in DEVICE_ENDPOINT_GUARDS.iter().zip(rows[1..].iter()) {
            let route = row["route"].as_str().unwrap();
            assert!(
                route.contains(guard.path),
                "{} missing from fixture row {}",
                guard.path,
                route
            );
            assert!(
                route.contains(guard.method),
                "{} missing from fixture row {}",
                guard.method,
                route
            );
            assert_eq!(guard.permission, row["perm"].as_str().unwrap());
            let auth: Vec<&str> = row["auth"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            // The workspaces row annotates the class with its token
            // semantics (`APIKeyAuthentication (APIToken ...)`), so compare
            // by prefix: every fixture entry must start with our class name.
            assert_eq!(
                guard.authentication.len(),
                auth.len(),
                "auth arity for {}",
                guard.name
            );
            for (ours, theirs) in guard.authentication.iter().zip(auth.iter()) {
                assert!(
                    theirs.starts_with(ours),
                    "auth for {}: {theirs:?} does not start with {ours:?}",
                    guard.name
                );
            }
            match (guard.throttle, row.get("throttle")) {
                (Some(scope), Some(v)) => assert_eq!(scope, v.as_str().unwrap()),
                (None, None) => {}
                (t, v) => panic!("throttle mismatch for {}: {t:?} vs {v:?}", guard.name),
            }
        }
    }

    #[test]
    fn oauth_view_shape_is_plain_django_get_with_session_keys() {
        let rows = f9()["endpoint_matrix"].as_array().unwrap().to_vec();
        let oauth = &rows[0];
        assert_eq!(oauth["perm"].as_str().unwrap(), "none (plain Django View)");
        assert!(oauth["auth"].as_array().unwrap().is_empty());
        assert!(oauth["method"].as_str().unwrap().contains("GET"));
        assert_eq!(OAUTH_VIEW_BASE, "django.views.View");
        assert_eq!(OAUTH_VIEW_METHOD, "GET");
        // App initiates (all four) + space gitea write the triple.
        for provider in [
            OauthProvider::Google,
            OauthProvider::Github,
            OauthProvider::Gitlab,
            OauthProvider::Gitea,
        ] {
            assert_eq!(
                initiate_session_keys(false, provider),
                &OAUTH_APP_INITIATE_SESSION_KEYS
            );
        }
        assert_eq!(
            initiate_session_keys(true, OauthProvider::Gitea),
            &OAUTH_SPACE_GITEA_INITIATE_SESSION_KEYS
        );
        assert_eq!(
            OAUTH_SPACE_GITEA_INITIATE_SESSION_KEYS, OAUTH_APP_INITIATE_SESSION_KEYS,
            "space gitea writes the same triple as app"
        );
        // Space google/github/gitlab omit next_path (ported as-is).
        for provider in [
            OauthProvider::Google,
            OauthProvider::Github,
            OauthProvider::Gitlab,
        ] {
            assert_eq!(
                initiate_session_keys(true, provider),
                &OAUTH_SPACE_INITIATE_SESSION_KEYS
            );
        }
        assert_eq!(OAUTH_CALLBACK_SESSION_KEYS, ["host", "next_path", "state"]);
    }
}
