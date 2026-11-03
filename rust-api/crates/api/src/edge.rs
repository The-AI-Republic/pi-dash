//! Cutover edge: routing table, per-prefix flags, web handlers, Django proxy.
//!
//! The Rust binary sits in front of Django. [`PREFIX_TABLE`] encodes the URL
//! prefix map (§0 of the inventory, from `apps/api/pi_dash/urls.py`); every
//! row has a flip flag ([`Prefix`]) that defaults to Django. Only routes with
//! a local handler are ever served by Rust — everything else, including
//! unflipped prefixes, is reverse-proxied to [`EdgeHandle::upstream`].
//!
//! The first Rust-owned prefix is `web`: `GET /` and `GET /robots.txt`,
//! byte-identical to `pi_dash.web.views` (see the `web_edge` contract suite).
//! The canonical handlers live in [`crate::web`]; this module keeps the
//! routing table, flags, proxy, and the shared serve-or-proxy helper.
//! Unsafe methods on those paths still proxy: Django's CSRF-failure page
//! embeds the deployment root URL, so Rust cannot reproduce it byte for byte.

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::state::AppState;

/// Upstream Django used when no flag is set. Matches Django's dev default.
pub const DEFAULT_UPSTREAM: &str = "http://127.0.0.1:8000";
/// Env var for the Django upstream base URL (no trailing slash).
pub const UPSTREAM_ENV: &str = "PIDASH_DJANGO_UPSTREAM";
/// Prefix of every per-prefix flip flag, e.g. `PIDASH_RUST_WEB`.
pub const FLAG_ENV_PREFIX: &str = "PIDASH_RUST_";
/// Proxy timeout. Mirrors gunicorn's 30s worker timeout.
const PROXY_TIMEOUT: Duration = Duration::from_secs(30);

/// Exact web-edge bytes, canonical home [`crate::web`] (D-00 handlers).
/// Re-exported here so existing `edge::HEALTH_BODY` paths keep working.
pub use crate::web::{HEALTH_BODY, ROBOTS_BODY};

/// One flippable row of the §0 prefix map. `api/` is shared by four rows;
/// all four must be off for the prefix to stay on Django.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prefix {
    Web,
    App,
    Assistant,
    Loop,
    Prompting,
    Space,
    License,
    RunnerWeb,
    ApiV1,
    Runner,
    Auth,
}

impl Prefix {
    /// All prefixes, in flag-registration order.
    pub const ALL: [Prefix; 11] = [
        Prefix::Web,
        Prefix::App,
        Prefix::Assistant,
        Prefix::Loop,
        Prefix::Prompting,
        Prefix::Space,
        Prefix::License,
        Prefix::RunnerWeb,
        Prefix::ApiV1,
        Prefix::Runner,
        Prefix::Auth,
    ];

    /// Suffix of the `PIDASH_RUST_*` env var holding this prefix's flag.
    pub fn env_suffix(self) -> &'static str {
        match self {
            Prefix::Web => "WEB",
            Prefix::App => "APP",
            Prefix::Assistant => "ASSISTANT",
            Prefix::Loop => "LOOP",
            Prefix::Prompting => "PROMPTING",
            Prefix::Space => "SPACE",
            Prefix::License => "LICENSE",
            Prefix::RunnerWeb => "RUNNER_WEB",
            Prefix::ApiV1 => "API_V1",
            Prefix::Runner => "RUNNER",
            Prefix::Auth => "AUTH",
        }
    }
}

/// The §0 URL-prefix map: Django mount prefix (no leading slash; `""` is the
/// site root that also acts as the catch-all) plus its Django module, keyed
/// to the flip flag. `ws/runner/` is absent: the Channels consumer is a
/// reject-stub, not an HTTP route, so there is nothing to proxy.
pub const PREFIX_TABLE: &[(&str, Prefix, &str)] = &[
    ("api/v1/runner/", Prefix::Runner, "runner.urls"),
    ("api/v1/", Prefix::ApiV1, "api.urls"),
    ("api/public/", Prefix::Space, "space.urls"),
    ("api/instances/", Prefix::License, "license.urls"),
    ("api/runners/", Prefix::RunnerWeb, "runner.web_urls"),
    ("api/", Prefix::App, "app.urls"),
    ("api/", Prefix::Assistant, "assistant.urls"),
    ("api/", Prefix::Loop, "loop.urls"),
    ("api/", Prefix::Prompting, "prompting.urls"),
    ("auth/", Prefix::Auth, "authentication.urls"),
    ("", Prefix::Web, "web.urls"),
];

/// Longest-prefix match of a request path against [`PREFIX_TABLE`].
/// Returns every row tied at the longest length (the four `api/` rows tie).
/// The path keeps its leading `/`; matching strips it.
pub fn match_prefixes(path: &str) -> Vec<Prefix> {
    let stripped = path.strip_prefix('/').unwrap_or(path);
    let mut best_len = 0usize;
    let mut best = Vec::new();
    for (prefix, key, _) in PREFIX_TABLE {
        if stripped.starts_with(prefix) && prefix.len() >= best_len {
            if prefix.len() > best_len {
                best_len = prefix.len();
                best.clear();
            }
            best.push(*key);
        }
    }
    best
}

/// Per-prefix flip flags. Everything defaults off: all traffic proxies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeFlags {
    flags: [bool; 11],
}

impl EdgeFlags {
    /// All flags off: the whole site proxies to Django.
    pub fn all_off() -> Self {
        Self { flags: [false; 11] }
    }

    /// Read flags from `PIDASH_RUST_*`. Absent or anything but
    /// `1/true/yes/on` (case-insensitive) means off: a misspelled flag
    /// fails safe toward Django, the known-good backend.
    pub fn from_env() -> Self {
        let mut flags = Self::all_off();
        for prefix in Prefix::ALL {
            let name = format!("{FLAG_ENV_PREFIX}{}", prefix.env_suffix());
            flags.set(prefix, env_is_truthy(&name));
        }
        flags
    }

    /// All flags on. Only the shadow router uses this: the shadow dispatch
    /// answers "what would Rust serve if this prefix were flipped".
    pub fn all_on() -> Self {
        Self { flags: [true; 11] }
    }

    fn index(prefix: Prefix) -> usize {
        Prefix::ALL.iter().position(|p| *p == prefix).unwrap_or(0)
    }

    pub fn set(&mut self, prefix: Prefix, on: bool) {
        self.flags[Self::index(prefix)] = on;
    }

    pub fn is_rust(&self, prefix: Prefix) -> bool {
        self.flags[Self::index(prefix)]
    }

    /// True when any matched prefix is flipped to Rust.
    pub fn any_rust(&self, prefixes: &[Prefix]) -> bool {
        prefixes.iter().any(|p| self.is_rust(*p))
    }
}

/// The shared truthy rule for `PIDASH_RUST_*` and the shadow flags:
/// `1/true/yes/on` (case-insensitive) is on, everything else (including
/// absent) is off.
pub(crate) fn env_is_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[derive(Debug)]
struct EdgeState {
    upstream: String,
    client: reqwest::Client,
    flags: RwLock<EdgeFlags>,
    /// True for handles derived by [`EdgeHandle::shadow_side`]: the proxy
    /// refuses to run under these (a marker instead of a second Django
    /// hit), which is how the shadow dispatch reports "no Rust handler".
    shadow_side: bool,
}

/// Shared cutover state: Django upstream, proxy client, live flags.
/// Cloned into every handler; [`set_flag`](EdgeHandle::set_flag) flips
/// routing at runtime, which is what the rollback drill exercises.
#[derive(Debug, Clone)]
pub struct EdgeHandle {
    inner: Arc<EdgeState>,
}

impl EdgeHandle {
    /// Build from explicit parts. `upstream` must be a base URL with no
    /// trailing slash, e.g. `http://127.0.0.1:8000`.
    pub fn new(upstream: impl Into<String>, flags: EdgeFlags) -> Result<Self, reqwest::Error> {
        // A transparent proxy must never follow redirects itself: the
        // upstream 3xx (status, Location, Set-Cookie, body) is forwarded
        // as-is so the original caller sees it (PIDASHCONV-226: Django's
        // POST /auth/sign-in/ 302 carries the session cookie).
        let client = reqwest::Client::builder()
            .timeout(PROXY_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            inner: Arc::new(EdgeState {
                upstream: upstream.into().trim_end_matches('/').to_owned(),
                client,
                flags: RwLock::new(flags),
                shadow_side: false,
            }),
        })
    }

    /// Derive a shadow-side handle sharing this upstream and client, with
    /// all flags on. Gated handlers serve their Rust response under it; the
    /// proxy itself refuses to run under it (see [`Self::is_shadow_side`]).
    pub fn shadow_side(&self) -> Self {
        Self {
            inner: Arc::new(EdgeState {
                upstream: self.inner.upstream.clone(),
                client: self.inner.client.clone(),
                flags: RwLock::new(EdgeFlags::all_on()),
                shadow_side: true,
            }),
        }
    }

    /// True for [`Self::shadow_side`] derivations: proxying under one would
    /// be a second Django hit for a request Django already answered, so
    /// [`proxy_request`] answers a shadow marker instead.
    pub fn is_shadow_side(&self) -> bool {
        self.inner.shadow_side
    }

    /// All flags off, pointing at [`DEFAULT_UPSTREAM`].
    pub fn for_tests(upstream: impl Into<String>) -> Self {
        Self::new(upstream, EdgeFlags::all_off()).expect("test edge handle")
    }

    /// Read upstream + flags from the environment.
    pub fn from_env() -> Result<Self, reqwest::Error> {
        let upstream = std::env::var(UPSTREAM_ENV).unwrap_or_else(|_| DEFAULT_UPSTREAM.to_owned());
        Self::new(upstream, EdgeFlags::from_env())
    }

    pub fn upstream(&self) -> &str {
        &self.inner.upstream
    }

    pub fn flags(&self) -> EdgeFlags {
        *self.inner.flags.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Flip one prefix at runtime. Rollback is `set_flag(prefix, false)`.
    pub fn set_flag(&self, prefix: Prefix, on: bool) {
        if let Ok(mut flags) = self.inner.flags.write() {
            flags.set(prefix, on);
        }
    }

    fn target(&self, path_and_query: &str) -> String {
        format!("{}{}", self.inner.upstream, path_and_query)
    }
}

/// Serve one Rust-owned web-edge response, or proxy when Django owns it.
/// Shared by the canonical D-00 handlers in [`crate::web`]: owned only
/// while the web prefix is flipped and only for `GET`/`HEAD`; every other
/// method, and every request while unflipped, proxies so Django's exact
/// behavior (including the CSRF-failure page) is preserved.
pub(crate) async fn serve_web_bytes(
    state: &AppState,
    req: Request,
    body: &'static [u8],
    content_type: &'static str,
) -> Response {
    let edge = state.edge();
    let owned =
        edge.flags().is_rust(Prefix::Web) && matches!(*req.method(), Method::GET | Method::HEAD);
    if !owned {
        // Through `proxy` (not `proxy_request` directly) so the answer
        // carries `ProxiedUpstream` like every other proxied arm — the
        // session layer stays transparent on marked responses
        // (PIDASHCONV-1304).
        return proxy(State(state.clone()), req).await;
    }
    let mut response = ([(header::CONTENT_TYPE, content_type)], body).into_response();
    if req.method() == Method::HEAD {
        // `any()` routes do not get axum's automatic HEAD stripping.
        *response.body_mut() = axum::body::Body::empty();
    }
    response
}

/// Marker: this response crossed the Django reverse proxy, so its
/// headers are already Django's own bytes (a view's `Allow` where
/// Django stamps one, none on framework pages such as the resolver
/// 404). View wrappers that stamp DRF headers must pass marked
/// responses through untouched to stay byte-identical.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProxiedUpstream;

/// Fallback: reverse-proxy to Django.
pub async fn proxy(State(state): State<AppState>, req: Request) -> Response {
    let mut response = proxy_request(&state, req).await;
    response.extensions_mut().insert(ProxiedUpstream);
    response
}

/// An owned path: the `owned` methods serve from `methods` as built,
/// everything else falls through to Django ([`proxy`]).
///
/// Canonical home of the per-module `owned` cutover helper
/// (PIDASHCONV-1593): every other copy is a call to this. `owned`
/// lists the methods to leave untouched — served by `methods`, or by
/// axum's default when `methods` does not register them. All eight of
/// `GET POST PUT PATCH DELETE OPTIONS HEAD TRACE` outside the list
/// route to [`proxy`], so `HEAD` and `TRACE` proxy explicitly: axum's
/// implicit `HEAD`-from-`GET` and its empty 405 would otherwise
/// diverge from Django (no `head` on these views, and `TRACE` 405s
/// as JSON everywhere).
pub fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in [
        "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE",
    ] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(proxy),
            "POST" => router.post(proxy),
            "PUT" => router.put(proxy),
            "PATCH" => router.patch(proxy),
            "DELETE" => router.delete(proxy),
            "HEAD" => router.head(proxy),
            "TRACE" => router.trace(proxy),
            _ => router.options(proxy),
        };
    }
    router
}

async fn proxy_request(state: &AppState, req: Request) -> Response {
    // A shadow dispatch that reaches the proxy (an unowned method on an
    // otherwise owned path) means "no Rust handler for this method": mark
    // it, never hit Django a second time.
    if state.edge().is_shadow_side() {
        return crate::edge_shadow::no_handler_response(
            crate::edge_shadow::NoHandlerReason::Proxied,
        );
    }
    // Shadow eligibility is decided before proxying; `None` (including
    // "shadow disabled") keeps the code path below exactly today's.
    let shadow = crate::edge_shadow::precheck(state, &req);
    let shadow_headers = shadow.as_ref().map(|_| req.headers().clone());
    let shadow_version = shadow.as_ref().map(|_| req.version());
    let edge = state.edge();
    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    let mut outgoing = edge
        .inner
        .client
        .request(method.clone(), edge.target(&path_and_query));

    let mut forwarded = HeaderMap::new();
    for (name, value) in req.headers() {
        if is_hop_by_hop(name) {
            continue;
        }
        forwarded.append(name, value.clone());
    }
    // The client's Host is forwarded unchanged (Caddy parity): Django
    // builds client-visible URLs and security checks from
    // `request.get_host()` and never reads `X-Forwarded-Host` (no
    // `USE_X_FORWARDED_HOST` under `apps/api/pi_dash/settings/`), so
    // letting reqwest re-derive Host from the upstream URL breaks OAuth
    // `redirect_uri`s, MinIO presigned URLs and the webhook self-host
    // guard (PIDASHCONV-1303). Requests without a Host header (HTTP/1.0)
    // still fall back to reqwest's URL-derived Host.
    // Present only when served with `into_make_service_with_connect_info`
    // (as `serve` does); absent in tests, where no forwarding headers apply.
    if let Some(peer) = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
    {
        append_forwarded_for(&mut forwarded, peer);
        if !forwarded.contains_key("x-forwarded-proto") {
            forwarded.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        }
        if !forwarded.contains_key("x-forwarded-host") {
            if let Some(host) = req.headers().get(header::HOST).cloned() {
                forwarded.insert("x-forwarded-host", host);
            }
        }
    }
    outgoing = outgoing.headers(forwarded);

    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => return bad_gateway(),
    };
    // The shadow re-dispatch replays the same body Django just received.
    let shadow_body = shadow.as_ref().map(|_| body_bytes.clone());
    outgoing = outgoing.body(body_bytes);

    let upstream = match outgoing.send().await {
        Ok(response) => response,
        Err(_) => return bad_gateway(),
    };

    if let (Some(plan), Some(headers), Some(version), Some(body)) =
        (shadow, shadow_headers, shadow_version, shadow_body)
    {
        return crate::edge_shadow::respond_with_shadow(
            plan,
            upstream,
            crate::edge_shadow::ShadowRequestParts {
                method,
                path_and_query,
                version,
                headers,
                body,
            },
        )
        .await;
    }

    let status = upstream.status();
    let headers = upstream.headers().clone();
    forward_upstream_response(
        status,
        &headers,
        axum::body::Body::from_stream(upstream.bytes_stream()),
    )
}

/// Build the client response from an upstream status, headers, and body.
/// The plain and shadow-teed proxy paths share this funnel, so the client
/// can never observe which path served it.
pub(crate) fn forward_upstream_response(
    status: StatusCode,
    headers: &HeaderMap,
    body: axum::body::Body,
) -> Response {
    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        if is_hop_by_hop(name) {
            continue;
        }
        response = response.header(name, value);
    }
    match response.body(body) {
        Ok(response) => response,
        Err(_) => bad_gateway(),
    }
}

fn append_forwarded_for(headers: &mut HeaderMap, peer: std::net::IpAddr) {
    let peer = peer.to_string();
    let value = match headers.get("x-forwarded-for") {
        Some(existing) => match existing.to_str() {
            Ok(list) => format!("{list}, {peer}"),
            Err(_) => peer,
        },
        None => peer,
    };
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.insert("x-forwarded-for", value);
    }
}

/// RFC 9110 §7.6.1 hop-by-hop headers are connection-scoped and must never
/// be relayed. `host` is handled separately: reqwest sets it from the URL.
fn is_hop_by_hop(name: &header::HeaderName) -> bool {
    *name == header::CONNECTION
        || *name == header::PROXY_AUTHENTICATE
        || *name == header::PROXY_AUTHORIZATION
        || *name == header::TE
        || *name == header::TRAILER
        || *name == header::TRANSFER_ENCODING
        || *name == header::UPGRADE
        || *name == header::HeaderName::from_static("keep-alive")
}

/// Exact body [`bad_gateway`] renders, shared so `Denial`-typed handlers
/// map a truncated request-body read to the same 502 bytes (PIDASHCONV-1308).
pub(crate) const BAD_GATEWAY_BODY: &str =
    r#"{"error":{"code":"bad_gateway","message":"bad gateway: django upstream unreachable"}}"#;

/// Fail-closed 502 in the shared error shape. Also used by the F-08
/// middleware layers when a proxied body truncates mid-read.
pub(crate) fn bad_gateway() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": {
                "code": "bad_gateway",
                "message": "bad gateway: django upstream unreachable",
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bad_gateway_renders_the_shared_body() {
        // `BAD_GATEWAY_BODY` is the const `Denial` arms reuse for the
        // truncated-body 502; it must stay byte-identical to this render.
        let response = bad_gateway();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        assert_eq!(bytes.as_ref(), BAD_GATEWAY_BODY.as_bytes());
    }

    #[test]
    fn table_covers_every_inventory_prefix() {
        let prefixes: Vec<&str> = PREFIX_TABLE.iter().map(|(p, _, _)| *p).collect();
        for expected in [
            "api/v1/runner/",
            "api/v1/",
            "api/public/",
            "api/instances/",
            "api/runners/",
            "api/",
            "auth/",
            "",
        ] {
            assert!(prefixes.contains(&expected), "missing prefix {expected:?}");
        }
        assert_eq!(
            PREFIX_TABLE.iter().filter(|(p, _, _)| *p == "api/").count(),
            4
        );
    }

    #[test]
    fn longest_prefix_wins() {
        assert_eq!(match_prefixes("/api/v1/runner/jobs"), vec![Prefix::Runner]);
        assert_eq!(match_prefixes("/api/v1/tokens"), vec![Prefix::ApiV1]);
        assert_eq!(match_prefixes("/api/public/boards"), vec![Prefix::Space]);
        assert_eq!(
            match_prefixes("/api/instances/config"),
            vec![Prefix::License]
        );
        assert_eq!(
            match_prefixes("/api/runners/sessions"),
            vec![Prefix::RunnerWeb]
        );
        assert_eq!(match_prefixes("/auth/login"), vec![Prefix::Auth]);
        assert_eq!(match_prefixes("/robots.txt"), vec![Prefix::Web]);
        assert_eq!(match_prefixes("/"), vec![Prefix::Web]);
        assert_eq!(match_prefixes("/unknown/path"), vec![Prefix::Web]);
    }

    #[test]
    fn shared_api_prefix_returns_all_four_rows() {
        assert_eq!(
            match_prefixes("/api/issues"),
            vec![
                Prefix::App,
                Prefix::Assistant,
                Prefix::Loop,
                Prefix::Prompting
            ]
        );
    }

    #[test]
    fn flags_default_off_and_flip_independently() {
        let mut flags = EdgeFlags::all_off();
        assert!(!flags.any_rust(&match_prefixes("/robots.txt")));
        flags.set(Prefix::Web, true);
        assert!(flags.any_rust(&match_prefixes("/")));
        assert!(!flags.any_rust(&match_prefixes("/api/v1/x")));
        flags.set(Prefix::Web, false);
        assert!(!flags.any_rust(&match_prefixes("/")));
    }

    #[test]
    fn shared_prefix_needs_one_of_four_flags() {
        let mut flags = EdgeFlags::all_off();
        let matched = match_prefixes("/api/issues");
        assert!(!flags.any_rust(&matched));
        flags.set(Prefix::Loop, true);
        assert!(flags.any_rust(&matched));
    }

    #[test]
    fn web_handlers_emit_django_exact_bytes() {
        assert_eq!(HEALTH_BODY, b"{\"status\": \"OK\"}");
        assert_eq!(ROBOTS_BODY, b"User-agent: *\nDisallow: /");
    }

    #[test]
    fn flags_from_env_default_off_and_parse_truthy() {
        for prefix in Prefix::ALL {
            std::env::remove_var(format!("{FLAG_ENV_PREFIX}{}", prefix.env_suffix()));
        }
        assert_eq!(EdgeFlags::from_env(), EdgeFlags::all_off());

        std::env::set_var("PIDASH_RUST_WEB", "1");
        std::env::set_var("PIDASH_RUST_API_V1", "true");
        std::env::set_var("PIDASH_RUST_AUTH", "banana");
        let flags = EdgeFlags::from_env();
        assert!(flags.is_rust(Prefix::Web));
        assert!(flags.is_rust(Prefix::ApiV1));
        assert!(!flags.is_rust(Prefix::Auth));
        assert!(!flags.is_rust(Prefix::App));

        for prefix in Prefix::ALL {
            std::env::remove_var(format!("{FLAG_ENV_PREFIX}{}", prefix.env_suffix()));
        }
    }

    #[test]
    fn upstream_normalizes_trailing_slash() {
        let edge = EdgeHandle::for_tests("http://django:8000/");
        assert_eq!(edge.upstream(), "http://django:8000");
        assert_eq!(edge.target("/?a=1"), "http://django:8000/?a=1");
    }

    #[tokio::test]
    async fn owned_serves_listed_and_proxies_the_rest() {
        use tower::ServiceExt;

        async fn hello() -> &'static str {
            "hello"
        }

        // Unreachable upstream (port 1 is never bound): proxied methods
        // fail closed with 502 (the `space::social` cutover-test shape).
        let state = AppState::with_edge("0.1.0", EdgeHandle::for_tests("http://127.0.0.1:1"));
        let app = crate::routes::with_routes(
            state,
            axum::Router::new()
                .route("/get-only", owned(axum::routing::get(hello), &["GET"]))
                .route("/post-only", owned(axum::routing::post(hello), &["POST"])),
        );
        async fn status_of(app: axum::Router, method: &str, path: &str) -> StatusCode {
            app.oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve")
            .status()
        }

        // The listed method serves from Rust.
        assert_eq!(
            status_of(app.clone(), "GET", "/get-only").await,
            StatusCode::OK
        );
        assert_eq!(
            status_of(app.clone(), "POST", "/post-only").await,
            StatusCode::OK
        );
        // Every other of the eight proxies and fails closed — `HEAD`
        // and `TRACE` explicitly, never axum's implicit-HEAD-from-`GET`
        // or its empty 405.
        for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE"] {
            assert_eq!(
                status_of(app.clone(), method, "/get-only").await,
                StatusCode::BAD_GATEWAY,
                "unowned {method} must proxy"
            );
        }
        for method in ["GET", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE"] {
            assert_eq!(
                status_of(app.clone(), method, "/post-only").await,
                StatusCode::BAD_GATEWAY,
                "unowned {method} must proxy"
            );
        }
    }
}
