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

/// Exact bytes Django renders for `GET /` (`JsonResponse({"status": "OK"})`).
pub const HEALTH_BODY: &[u8] = b"{\"status\": \"OK\"}";
/// Exact bytes Django renders for `GET /robots.txt`.
pub const ROBOTS_BODY: &[u8] = b"User-agent: *\nDisallow: /";

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
            let on = std::env::var(&name)
                .map(|v| {
                    matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "on"
                    )
                })
                .unwrap_or(false);
            flags.set(prefix, on);
        }
        flags
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

#[derive(Debug)]
struct EdgeState {
    upstream: String,
    client: reqwest::Client,
    flags: RwLock<EdgeFlags>,
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
        let client = reqwest::Client::builder().timeout(PROXY_TIMEOUT).build()?;
        Ok(Self {
            inner: Arc::new(EdgeState {
                upstream: upstream.into().trim_end_matches('/').to_owned(),
                client,
                flags: RwLock::new(flags),
            }),
        })
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

/// `GET /` when the web prefix is flipped; every other method, and every
/// request while unflipped, proxies so Django's exact behavior (including
/// the CSRF-failure page) is preserved.
pub async fn web_root(State(state): State<AppState>, req: Request) -> Response {
    edge_endpoint(&state, req, HEALTH_BODY, "application/json").await
}

/// `GET /robots.txt`, same ownership rule as [`web_root`].
pub async fn web_robots(State(state): State<AppState>, req: Request) -> Response {
    edge_endpoint(&state, req, ROBOTS_BODY, "text/plain").await
}

async fn edge_endpoint(
    state: &AppState,
    req: Request,
    body: &'static [u8],
    content_type: &'static str,
) -> Response {
    let edge = state.edge();
    let owned =
        edge.flags().is_rust(Prefix::Web) && matches!(*req.method(), Method::GET | Method::HEAD);
    if !owned {
        return proxy_request(state, req).await;
    }
    let mut response = ([(header::CONTENT_TYPE, content_type)], body).into_response();
    if req.method() == Method::HEAD {
        // `any()` routes do not get axum's automatic HEAD stripping.
        *response.body_mut() = axum::body::Body::empty();
    }
    response
}

/// Fallback: reverse-proxy to Django.
pub async fn proxy(State(state): State<AppState>, req: Request) -> Response {
    proxy_request(&state, req).await
}

async fn proxy_request(state: &AppState, req: Request) -> Response {
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
        if is_hop_by_hop(name) || *name == header::HOST {
            continue;
        }
        forwarded.append(name, value.clone());
    }
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

    match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => {
            outgoing = outgoing.body(bytes);
        }
        Err(_) => return bad_gateway(),
    }

    let upstream = match outgoing.send().await {
        Ok(response) => response,
        Err(_) => return bad_gateway(),
    };

    let mut response = Response::builder().status(upstream.status());
    for (name, value) in upstream.headers() {
        if is_hop_by_hop(name) {
            continue;
        }
        response = response.header(name, value);
    }
    match response.body(axum::body::Body::from_stream(upstream.bytes_stream())) {
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

fn bad_gateway() -> Response {
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
}
