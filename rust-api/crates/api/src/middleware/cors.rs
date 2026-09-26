#![forbid(unsafe_code)]

//! CORS handling, mirroring `corsheaders.middleware.CorsMiddleware`.
//!
//! Python references: `corsheaders/middleware.py` and
//! `corsheaders/defaults.py` (django-cors-headers 4.3.1), plus the project
//! settings in `apps/api/pi_dash/settings/common.py` (`CORS_ALLOW_CREDENTIALS
//! = True`, origins from `CORS_ALLOWED_ORIGINS` or allow-all, `X-API-Key`
//! appended to the default allow-headers).
//!
//! Rules reproduced exactly, in order:
//!
//! 1. The middleware is enabled for every path (`CORS_URLS_REGEX` keeps its
//!    default `^.*$`; the project never sets it, and there are no
//!    `check_request_enabled` signal receivers, so both checks collapse to
//!    "always on").
//! 2. A preflight — `OPTIONS` with an `Access-Control-Request-Method`
//!    header — short-circuits to an empty `200` (`content-length: 0`) before
//!    the inner service runs; the CORS headers below are still applied.
//! 3. `Vary: Origin` is patched whenever the middleware runs, even when the
//!    request carries no `Origin` header.
//! 4. Without an `Origin` header, with an unparseable origin, or — when
//!    origins are restricted — with an origin matching neither the allow
//!    list (scheme + netloc compare, plus a literal `"null"` entry) nor the
//!    (here empty) regex list, the response passes through untouched.
//! 5. With allow-all and no credentials the response carries `*`;
//!    otherwise it echoes the request origin. Credentials add
//!    `Access-Control-Allow-Credentials: true`.
//! 6. Every allowed `OPTIONS` request — preflight or not — also carries the
//!    allow-headers / allow-methods lists, plus `Access-Control-Max-Age`
//!    when set.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, HeaderValue, Method, Request, Response, StatusCode};
use tower::{Layer, Service};

/// django-cors-headers `default_methods`.
pub const DEFAULT_METHODS: &[&str] = &["DELETE", "GET", "OPTIONS", "PATCH", "POST", "PUT"];
/// django-cors-headers `default_headers`, with the project's `X-API-Key` addition.
pub const DEFAULT_HEADERS: &[&str] = &[
    "accept",
    "authorization",
    "content-type",
    "user-agent",
    "x-csrftoken",
    "x-requested-with",
    "X-API-Key",
];
/// django-cors-headers `CORS_PREFLIGHT_MAX_AGE` default.
pub const DEFAULT_PREFLIGHT_MAX_AGE: u64 = 86_400;

/// What the layer allows, after the F-03 `Settings` overlay is applied.
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// `CORS_ALLOW_ALL_ORIGINS`. True when `CORS_ALLOWED_ORIGINS` is empty.
    pub allow_all_origins: bool,
    /// `CORS_ALLOWED_ORIGINS`, as `scheme://netloc` strings.
    pub allowed_origins: Vec<String>,
    /// `CORS_ALLOW_CREDENTIALS`. The project sets `True`.
    pub allow_credentials: bool,
    /// `CORS_ALLOW_METHODS`. django-cors-headers defaults.
    pub allow_methods: Vec<String>,
    /// `CORS_ALLOW_HEADERS`. Defaults plus `X-API-Key`.
    pub allow_headers: Vec<String>,
    /// `CORS_EXPOSE_HEADERS`. Empty in this project.
    pub expose_headers: Vec<String>,
    /// `CORS_PREFLIGHT_MAX_AGE`. django-cors-headers default.
    pub preflight_max_age: u64,
}

impl CorsConfig {
    /// The deployment shape: credentials on, project allow-headers, nothing
    /// exposed, default methods and max age. Origins come from
    /// `CORS_ALLOWED_ORIGINS`; an empty list means allow-all, exactly like
    /// `common.py` (which sets `CORS_ALLOW_ALL_ORIGINS` only then).
    pub fn deployment(allowed_origins: Vec<String>) -> Self {
        Self {
            allow_all_origins: allowed_origins.is_empty(),
            allowed_origins,
            allow_credentials: true,
            allow_methods: DEFAULT_METHODS.iter().map(|s| s.to_string()).collect(),
            allow_headers: DEFAULT_HEADERS.iter().map(|s| s.to_string()).collect(),
            expose_headers: Vec::new(),
            preflight_max_age: DEFAULT_PREFLIGHT_MAX_AGE,
        }
    }

    /// Build from the F-03 `Settings` overlay. The allow-all flag is read
    /// from settings (which F-03 derives as `origins.is_empty()`, like
    /// `common.py`) rather than recomputed.
    pub fn from_settings(settings: &pidash_db::config::Settings) -> Self {
        let mut config = Self::deployment(settings.cors_allowed_origins.clone());
        config.allow_all_origins = settings.cors_allow_all_origins;
        config
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        if self.allow_all_origins {
            return true;
        }
        // A literal "null" origin only matches an explicit list entry,
        // verbatim from `origin_found_in_white_lists`.
        if origin == "null" {
            return self.allowed_origins.iter().any(|o| o == "null");
        }
        let Some((scheme, netloc)) = split_origin(origin) else {
            return false;
        };
        // `CORS_ALLOWED_ORIGIN_REGEXES` is empty in this project, and there
        // are no signal receivers, so only the scheme + netloc compare
        // remains.
        self.allowed_origins
            .iter()
            .any(|allowed| split_origin(allowed).is_some_and(|(s, n)| s == scheme && n == netloc))
    }
}

/// Split `scheme://netloc[/...]` into a lowercased scheme and the raw
/// netloc. `None` means "unparseable, reject the origin" (Python's
/// `urlsplit` raises `ValueError` on some of these; either way the origin
/// is rejected). The scheme is lowercased because `urlsplit` normalises it;
/// the netloc keeps its case like `SplitResult.netloc`.
fn split_origin(origin: &str) -> Option<(String, &str)> {
    let (scheme, rest) = origin.split_once("://")?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    let netloc = rest.split('/').next().unwrap_or(rest);
    if netloc.is_empty() {
        return None;
    }
    Some((scheme.to_ascii_lowercase(), netloc))
}

#[derive(Debug, Clone)]
pub struct CorsLayer {
    config: CorsConfig,
}

impl CorsLayer {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for CorsLayer {
    type Service = CorsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CorsService {
            inner,
            config: self.config.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CorsService<S> {
    inner: S,
    config: CorsConfig,
}

fn request_origin(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn apply_headers(
    config: &CorsConfig,
    origin: Option<&str>,
    method: &Method,
    response: &mut Response<Body>,
) {
    // Vary: Origin is patched whenever the middleware runs, even with no
    // Origin header on the request.
    crate::middleware::append_vary(response.headers_mut(), "Origin");

    let Some(origin) = origin.filter(|o| !o.is_empty()) else {
        return;
    };
    if !config.origin_allowed(origin) {
        return;
    }

    let headers = response.headers_mut();
    if config.allow_all_origins && !config.allow_credentials {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    } else if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    if config.allow_credentials {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
    }
    if !config.expose_headers.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.expose_headers.join(", ")) {
            headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, value);
        }
    }
    if method == Method::OPTIONS {
        if let Ok(value) = HeaderValue::from_str(&config.allow_headers.join(", ")) {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, value);
        }
        if let Ok(value) = HeaderValue::from_str(&config.allow_methods.join(", ")) {
            headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, value);
        }
        if config.preflight_max_age != 0 {
            headers.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from(config.preflight_max_age),
            );
        }
    }
}

impl<S> Service<Request<Body>> for CorsService<S>
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
        // `apply_headers` only needs the origin and the method, so stash
        // both before the request moves into the inner service.
        let origin = request_origin(&req);
        let method = req.method().clone();
        if method == Method::OPTIONS
            && req
                .headers()
                .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
        {
            // Preflight short-circuit: empty 200 with content-length 0;
            // CORS headers are still applied (check_preflight returns the
            // response, add_response_headers decorates it).
            let config = self.config.clone();
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_LENGTH, "0")
                .body(Body::empty())
                .expect("static preflight response");
            apply_headers(&config, origin.as_deref(), &method, &mut response);
            return Box::pin(async move { Ok(response) });
        }
        let config = self.config.clone();
        // Clone-and-swap keeps `call` taking `&mut self` while the future
        // owns its service, the tower-http convention.
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let mut response = inner.call(req).await?;
            apply_headers(&config, origin.as_deref(), &method, &mut response);
            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::{Layer, ServiceExt};

    /// Concrete inner service: `Router` (opaque `impl Service` returns
    /// cannot prove `S::Future: Send`, which the layer impls require).
    fn inner() -> axum::Router {
        axum::Router::new().route("/api/x/", axum::routing::any(|| async { "ok" }))
    }

    fn config() -> CorsConfig {
        CorsConfig::deployment(vec!["https://app.example.com".to_string()])
    }

    fn vary(response: &Response<Body>) -> String {
        response
            .headers()
            .get(header::VARY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    #[tokio::test]
    async fn preflight_short_circuits_with_cors_headers() {
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "https://app.example.com")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok()),
            Some("0")
        );
        let headers = response.headers();
        assert_eq!(
            headers
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://app.example.com")
        );
        assert_eq!(
            headers
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .and_then(|v| v.to_str().ok()),
            Some("true")
        );
        assert!(headers.contains_key(header::ACCESS_CONTROL_ALLOW_HEADERS));
        assert!(headers.contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
        assert_eq!(
            headers
                .get(header::ACCESS_CONTROL_MAX_AGE)
                .and_then(|v| v.to_str().ok()),
            Some("86400")
        );
        assert!(vary(&response).contains("Origin"));
    }

    #[tokio::test]
    async fn preflight_short_circuits_before_routing() {
        // Django answers preflights before URL resolution: even an
        // unroutable path gets the empty 200 with CORS headers.
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/no/such/path/")
                    .header(header::ORIGIN, "https://app.example.com")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://app.example.com")
        );
    }

    #[tokio::test]
    async fn preflight_from_disallowed_origin_has_no_cors_headers() {
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "https://evil.example.com")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        // Still short-circuited (check_preflight runs before the origin
        // check), but without allow headers.
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
        assert!(vary(&response).contains("Origin"));
    }

    #[tokio::test]
    async fn actual_request_echoes_allowed_origin() {
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "https://app.example.com")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let headers = response.headers();
        assert_eq!(
            headers
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://app.example.com")
        );
        assert_eq!(
            headers
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .and_then(|v| v.to_str().ok()),
            Some("true")
        );
        // Not OPTIONS: no allow-methods/allow-headers block.
        assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
    }

    #[tokio::test]
    async fn allow_all_with_credentials_echoes_origin_not_star() {
        let config = CorsConfig::deployment(Vec::new());
        assert!(config.allow_all_origins);
        let response = CorsLayer::new(config)
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "https://anything.example.com")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://anything.example.com")
        );
    }

    #[tokio::test]
    async fn allow_all_without_credentials_sends_star() {
        let mut config = CorsConfig::deployment(Vec::new());
        config.allow_credentials = false;
        let response = CorsLayer::new(config)
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "https://anything.example.com")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS));
    }

    #[tokio::test]
    async fn missing_origin_still_patches_vary() {
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::get("/api/x/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert!(vary(&response).contains("Origin"));
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    }

    #[tokio::test]
    async fn options_without_preflight_header_reaches_inner() {
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "https://app.example.com")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        // Not a preflight (no Access-Control-Request-Method): the inner
        // service ran, and the OPTIONS header block still applied.
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn origin_matching_ignores_case_on_scheme() {
        let response = CorsLayer::new(config())
            .layer(inner())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/x/")
                    .header(header::ORIGIN, "HTTPS://app.example.com")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("HTTPS://app.example.com")
        );
    }

    #[test]
    fn split_origin_rejects_garbage() {
        assert!(split_origin("not-an-origin").is_none());
        assert!(split_origin("https://").is_none());
        assert!(split_origin("").is_none());
        let (scheme, netloc) = split_origin("https://app.example.com:8443/api").expect("origin");
        assert_eq!(scheme, "https");
        assert_eq!(netloc, "app.example.com:8443");
    }
}
