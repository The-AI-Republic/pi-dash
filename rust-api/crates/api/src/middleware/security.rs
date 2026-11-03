#![forbid(unsafe_code)]

//! Security response headers, mirroring Django's `SecurityMiddleware` and
//! `XFrameOptionsMiddleware`.
//!
//! Python references: `django/middleware/security.py`,
//! `django/middleware/clickjacking.py`, and the defaults in
//! `django/conf/global_settings.py`. The project overrides none of the
//! relevant settings, so the deployed behavior is exactly the Django
//! defaults:
//!
//! - `SECURE_HSTS_SECONDS = 0`: no `Strict-Transport-Security` header.
//!   The config keeps the knob (with Django's `includeSubDomains` /
//!   `preload` defaults of false) so a future settings change has somewhere
//!   to land.
//! - `SECURE_CONTENT_TYPE_NOSNIFF = True`: `X-Content-Type-Options: nosniff`,
//!   set only when absent (`setdefault`).
//! - `SECURE_REFERRER_POLICY = "same-origin"`: `Referrer-Policy:
//!   same-origin`, set only when absent.
//! - `SECURE_CROSS_ORIGIN_OPENER_POLICY = "same-origin"`:
//!   `Cross-Origin-Opener-Policy: same-origin`, set only when absent.
//! - `X_FRAME_OPTIONS = "DENY"`: `X-Frame-Options: DENY`, set only when
//!   absent. (There is no `@xframe_options_exempt` concept in Rust yet:
//!   a handler that sets the header itself wins, which covers the same
//!   ground.)
//! - `SECURE_SSL_REDIRECT = False`: no http→https redirect. `is_secure()`
//!   still matters for HSTS, and honors `X-Forwarded-Proto` exactly when
//!   `production.py` sets `SECURE_PROXY_SSL_HEADER` (surfaced here as
//!   [`SecurityConfig::trust_forwarded_proto`], fed from the F-03
//!   `Settings.secure_proxy_ssl_header`).

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response};
use tower::{Layer, Service};

/// Which security headers to emit. [`SecurityConfig::django_defaults`]
/// reproduces the deployed Django behavior.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// `SECURE_HSTS_SECONDS`. Deployed as `0` (header off).
    pub hsts_seconds: u64,
    /// `SECURE_HSTS_INCLUDE_SUBDOMAINS`. Django default `False`.
    pub hsts_include_subdomains: bool,
    /// `SECURE_HSTS_PRELOAD`. Django default `False`.
    pub hsts_preload: bool,
    /// `SECURE_CONTENT_TYPE_NOSNIFF`. Django default `True`.
    pub content_type_nosniff: bool,
    /// `SECURE_REFERRER_POLICY`. Django default `"same-origin"`.
    /// `None` disables the header.
    pub referrer_policy: Option<String>,
    /// `SECURE_CROSS_ORIGIN_OPENER_POLICY`. Django default `"same-origin"`.
    /// `None` disables the header.
    pub cross_origin_opener_policy: Option<String>,
    /// `X_FRAME_OPTIONS`. Django default `"DENY"`.
    pub x_frame_options: String,
    /// Honor `X-Forwarded-Proto: https` for `is_secure()`. True only in the
    /// production settings (`SECURE_PROXY_SSL_HEADER`).
    pub trust_forwarded_proto: bool,
}

impl SecurityConfig {
    /// The deployed behavior: Django defaults plus the
    /// `trust_forwarded_proto` flag from settings.
    pub fn django_defaults(trust_forwarded_proto: bool) -> Self {
        Self {
            hsts_seconds: 0,
            hsts_include_subdomains: false,
            hsts_preload: false,
            content_type_nosniff: true,
            referrer_policy: Some("same-origin".to_string()),
            cross_origin_opener_policy: Some("same-origin".to_string()),
            // `get_xframe_options_value` uppercases the setting; the
            // project keeps the default, so this is already upper case.
            x_frame_options: "DENY".to_string(),
            trust_forwarded_proto,
        }
    }

    /// Build from the F-03 `Settings` overlay: Django defaults, with the
    /// production `SECURE_PROXY_SSL_HEADER` flag wired through.
    pub fn from_settings(settings: &pidash_db::config::Settings) -> Self {
        Self::django_defaults(settings.secure_proxy_ssl_header)
    }

    fn hsts_value(&self) -> Option<String> {
        if self.hsts_seconds == 0 {
            return None;
        }
        let mut value = format!("max-age={}", self.hsts_seconds);
        if self.hsts_include_subdomains {
            value.push_str("; includeSubDomains");
        }
        if self.hsts_preload {
            value.push_str("; preload");
        }
        Some(value)
    }
}

/// `request.is_secure()` for HSTS: https scheme, or `X-Forwarded-Proto:
/// https` when the deployment trusts the proxy header.
fn is_secure(req: &Request<Body>, trust_forwarded_proto: bool) -> bool {
    if req.uri().scheme_str() == Some("https") {
        return true;
    }
    trust_forwarded_proto
        && req
            .headers()
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

fn set_default(headers: &mut header::HeaderMap, name: header::HeaderName, value: &str) {
    if headers.contains_key(&name) {
        return;
    }
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

fn apply_headers(config: &SecurityConfig, secure: bool, headers: &mut header::HeaderMap) {
    // HSTS is set (not setdefault) whenever due, but never overwrites an
    // existing value — verbatim from process_response.
    if secure && !headers.contains_key(header::STRICT_TRANSPORT_SECURITY) {
        if let Some(value) = config.hsts_value() {
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.insert(header::STRICT_TRANSPORT_SECURITY, value);
            }
        }
    }
    if config.content_type_nosniff {
        set_default(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    }
    if let Some(policy) = &config.referrer_policy {
        // A comma-separated string or an iterable both collapse to a
        // comma-joined value; the config already holds the single string.
        set_default(headers, header::REFERRER_POLICY, policy);
    }
    if let Some(policy) = &config.cross_origin_opener_policy {
        set_default(
            headers,
            "cross-origin-opener-policy"
                .parse()
                .expect("static header name"),
            policy,
        );
    }
    set_default(headers, header::X_FRAME_OPTIONS, &config.x_frame_options);
}

#[derive(Debug, Clone)]
pub struct SecurityLayer {
    config: SecurityConfig,
}

impl SecurityLayer {
    pub fn new(config: SecurityConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for SecurityLayer {
    type Service = SecurityService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SecurityService {
            inner,
            config: self.config.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecurityService<S> {
    inner: S,
    config: SecurityConfig,
}

impl<S> Service<Request<Body>> for SecurityService<S>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let config = self.config.clone();
        let secure = is_secure(&req, config.trust_forwarded_proto);
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let mut response = inner.call(req).await?;
            apply_headers(&config, secure, response.headers_mut());
            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;
    use tower::{Layer, ServiceExt};

    /// Concrete inner service (see cors tests for why `Router`).
    fn inner() -> axum::Router {
        axum::Router::new()
            .route("/", axum::routing::any(|| async { "ok" }))
            .route("/api/x/", axum::routing::any(|| async { "ok" }))
    }

    fn header(response: &Response<Body>, name: header::HeaderName) -> Option<String> {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }

    #[tokio::test]
    async fn django_defaults_land_on_plain_response() {
        let response = SecurityLayer::new(SecurityConfig::django_defaults(false))
            .layer(inner())
            .oneshot(
                Request::get("http://app.example.com/api/x/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let headers = response.headers();
        assert_eq!(
            headers
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        assert_eq!(
            headers
                .get(header::REFERRER_POLICY)
                .and_then(|v| v.to_str().ok()),
            Some("same-origin")
        );
        assert_eq!(
            header(
                &response,
                "cross-origin-opener-policy".parse().expect("coop")
            ),
            Some("same-origin".to_string())
        );
        assert_eq!(
            headers
                .get(header::X_FRAME_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("DENY")
        );
        // HSTS off by default (SECURE_HSTS_SECONDS = 0).
        assert!(!headers.contains_key(header::STRICT_TRANSPORT_SECURITY));
    }

    #[tokio::test]
    async fn existing_headers_are_never_overwritten() {
        let inner = axum::Router::new().route(
            "/",
            axum::routing::any(|| async {
                (
                    [
                        (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
                        (header::X_CONTENT_TYPE_OPTIONS, "custom"),
                    ],
                    "ok",
                )
            }),
        );
        let response = SecurityLayer::new(SecurityConfig::django_defaults(false))
            .layer(inner)
            .oneshot(
                Request::get("http://app.example.com/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        // setdefault: handler-set values win.
        assert_eq!(
            response
                .headers()
                .get(header::X_FRAME_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("SAMEORIGIN")
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("custom")
        );
    }

    #[tokio::test]
    async fn hsts_needs_seconds_and_a_secure_request() {
        let mut config = SecurityConfig::django_defaults(false);
        config.hsts_seconds = 31_536_000;
        // Plain http: no HSTS.
        let response = SecurityLayer::new(config.clone())
            .layer(inner())
            .oneshot(
                Request::get("http://app.example.com/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert!(!response
            .headers()
            .contains_key(header::STRICT_TRANSPORT_SECURITY));
        // https: HSTS with Django's exact shape.
        let response = SecurityLayer::new(config.clone())
            .layer(inner())
            .oneshot(
                Request::get("https://app.example.com/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::STRICT_TRANSPORT_SECURITY)
                .and_then(|v| v.to_str().ok()),
            Some("max-age=31536000")
        );
        // includeSubDomains + preload flags render verbatim.
        config.hsts_include_subdomains = true;
        config.hsts_preload = true;
        let response = SecurityLayer::new(config)
            .layer(inner())
            .oneshot(
                Request::get("https://app.example.com/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::STRICT_TRANSPORT_SECURITY)
                .and_then(|v| v.to_str().ok()),
            Some("max-age=31536000; includeSubDomains; preload")
        );
    }

    #[tokio::test]
    async fn forwarded_proto_counts_when_trusted() {
        let mut config = SecurityConfig::django_defaults(true);
        config.hsts_seconds = 60;
        let response = SecurityLayer::new(config)
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("http://app.example.com/")
                    .header("x-forwarded-proto", "https")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::STRICT_TRANSPORT_SECURITY)
                .and_then(|v| v.to_str().ok()),
            Some("max-age=60")
        );
        // Untrusted: the same header is ignored (common.py behaviour).
        let mut config = SecurityConfig::django_defaults(false);
        config.hsts_seconds = 60;
        let response = SecurityLayer::new(config)
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("http://app.example.com/")
                    .header("x-forwarded-proto", "https")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert!(!response
            .headers()
            .contains_key(header::STRICT_TRANSPORT_SECURITY));
    }
}
