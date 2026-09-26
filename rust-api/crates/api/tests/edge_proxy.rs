//! Cutover edge integration tests (F-02).
//!
//! A stub "Django" upstream (plain axum) stands in for the real backend.
//! The edge app under test proxies to it or, when the web flag flips,
//! serves `/` and `/robots.txt` itself. Assertions mirror the `web_edge`
//! pytest oracle: exact bytes, exact content types, HEAD emptiness,
//! credential-indifference, and no cookies on Rust-served responses.

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use pidash_api::{with_routes, AppState, EdgeHandle, Prefix};

const HEALTH_BODY: &[u8] = b"{\"status\": \"OK\"}";
const ROBOTS_BODY: &[u8] = b"User-agent: *\nDisallow: /";
const CSRF_MARKER: &str = "<!-- templates/csrf_failure.html -->";

/// Stub Django: exact web-edge bytes plus echo/relay endpoints for the proxy.
async fn stub_app() -> axum::Router {
    async fn root(req: Request) -> Response {
        if req.method() == axum::http::Method::POST {
            // Django's CSRF_FAILURE_VIEW: 200 HTML page, embeds deploy URL.
            return (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                format!("<html>{CSRF_MARKER} root=https://stub.invalid</html>"),
            )
                .into_response();
        }
        (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            // Marked so tests can tell stub bytes from Rust-served bytes.
            &b"{\"status\": \"STUB\"}"[..],
        )
            .into_response()
    }
    async fn robots() -> impl IntoResponse {
        (
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            ROBOTS_BODY,
        )
    }
    async fn echo(req: Request) -> Response {
        let method = req.method().to_string();
        let path = req.uri().path().to_owned();
        let query = req.uri().query().unwrap_or("").to_owned();
        let headers = req.headers().clone();
        let body = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap_or_default();
        let x_custom = headers
            .get("x-custom")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let forwarded_for = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        axum::Json(serde_json::json!({
            "method": method,
            "path": path,
            "query": query,
            "body": String::from_utf8_lossy(&body),
            "x_custom": x_custom,
            "x_forwarded_for": forwarded_for,
        }))
        .into_response()
    }
    async fn relay(_headers: HeaderMap) -> Response {
        let mut response = (StatusCode::CREATED, "created").into_response();
        response.headers_mut().append(
            axum::http::header::SET_COOKIE,
            "a=1; Path=/".parse().expect("cookie"),
        );
        response.headers_mut().append(
            axum::http::header::SET_COOKIE,
            "b=2; Path=/".parse().expect("cookie"),
        );
        response
            .headers_mut()
            .append("x-multi", "one".parse().expect("header"));
        response
            .headers_mut()
            .append("x-multi", "two".parse().expect("header"));
        response
    }
    axum::Router::new()
        .route("/", any(root))
        .route("/robots.txt", any(robots))
        .route("/api/v1/things", any(echo))
        .route("/api/v1/relay", any(relay))
        .fallback((StatusCode::NOT_FOUND, "stub 404"))
}

async fn spawn(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

struct Edge {
    base: String,
    handle: EdgeHandle,
    _task: tokio::task::JoinHandle<()>,
}

async fn spawn_edge(upstream: &str) -> Edge {
    use std::net::SocketAddr;

    let handle = EdgeHandle::for_tests(upstream);
    let app = with_routes(
        AppState::with_edge("test", handle.clone()),
        axum::Router::new(),
    );
    // Same wiring as `serve`: inject the peer address so the proxy can set
    // `x-forwarded-for` (the stub-only `spawn` helper skips this).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });
    Edge {
        base: format!("http://127.0.0.1:{port}"),
        handle,
        _task: task,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn flag_off_proxies_web_edge_to_upstream() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base).await;
    let client = client();

    // Unflipped: everything comes from the stub, byte for byte.
    let root = client
        .get(format!("{}/", edge.base))
        .send()
        .await
        .expect("get /");
    assert_eq!(root.status(), 200);
    assert_eq!(root.headers()["content-type"], "application/json");
    assert_eq!(
        root.bytes().await.expect("body").as_ref(),
        b"{\"status\": \"STUB\"}"
    );

    let robots = client
        .get(format!("{}/robots.txt", edge.base))
        .send()
        .await
        .expect("get robots");
    assert_eq!(robots.status(), 200);
    assert_eq!(robots.headers()["content-type"], "text/plain");
    assert_eq!(robots.bytes().await.expect("body").as_ref(), ROBOTS_BODY);
}

#[tokio::test]
async fn flag_on_serves_web_edge_without_upstream() {
    // Upstream points at a closed port: any proxy attempt would 502, so a
    // 200 with exact bytes proves Rust served the response itself.
    let edge = spawn_edge("http://127.0.0.1:1").await;
    edge.handle.set_flag(Prefix::Web, true);
    let client = client();

    for (path, body, content_type) in [
        ("/", HEALTH_BODY, "application/json"),
        ("/robots.txt", ROBOTS_BODY, "text/plain"),
    ] {
        let response = client
            .get(format!("{}{}", edge.base, path))
            .send()
            .await
            .expect("get");
        assert_eq!(response.status(), 200, "path {path}");
        assert_eq!(response.headers()["content-type"], content_type);
        assert_eq!(response.bytes().await.expect("body").as_ref(), body);
        // Rust-served edge responses carry no identity: no cookies, and
        // foreign credentials change nothing.
        let foreign = client
            .get(format!("{}{}", edge.base, path))
            .header("Authorization", "Bearer deadbeef")
            .header("Cookie", "sessionid=deadbeef")
            .send()
            .await
            .expect("get foreign");
        assert_eq!(foreign.status(), 200);
        assert!(!foreign.headers().contains_key("set-cookie"));
        assert_eq!(foreign.bytes().await.expect("body").as_ref(), body);
    }

    // Non-owned paths still proxy, hence 502 against the closed port.
    let relay = client
        .get(format!("{}/api/v1/things", edge.base))
        .send()
        .await
        .expect("get");
    assert_eq!(relay.status(), 502);
}

#[tokio::test]
async fn head_requests_have_no_body_when_flipped() {
    let edge = spawn_edge("http://127.0.0.1:1").await;
    edge.handle.set_flag(Prefix::Web, true);
    let client = client();

    for path in ["/", "/robots.txt"] {
        let response = client
            .head(format!("{}{}", edge.base, path))
            .send()
            .await
            .expect("head");
        assert_eq!(response.status(), 200, "path {path}");
        assert_eq!(response.bytes().await.expect("body").as_ref(), b"");
    }
}

#[tokio::test]
async fn unsafe_methods_proxy_even_when_flipped() {
    // Django answers POST without a CSRF token with its failure page (200,
    // deployment-specific body). Rust must not shadow that behavior.
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base).await;
    edge.handle.set_flag(Prefix::Web, true);
    let client = client();

    let response = client
        .post(format!("{}/", edge.base))
        .send()
        .await
        .expect("post");
    assert_eq!(response.status(), 200);
    assert!(response.headers()["content-type"]
        .to_str()
        .expect("ct")
        .starts_with("text/html"));
    let body = response.bytes().await.expect("body");
    assert!(body
        .windows(CSRF_MARKER.len())
        .any(|w| w == CSRF_MARKER.as_bytes()));
}

#[tokio::test]
async fn proxy_relays_method_path_query_body_and_headers() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base).await;
    let client = client();

    let response = client
        .post(format!("{}/api/v1/things?id=7&tag=a", edge.base))
        .header("X-Custom", "hello")
        .body("payload-bytes")
        .send()
        .await
        .expect("post");
    assert_eq!(response.status(), 200);
    let body = response.bytes().await.expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["method"], "POST");
    assert_eq!(json["path"], "/api/v1/things");
    assert_eq!(json["query"], "id=7&tag=a");
    assert_eq!(json["body"], "payload-bytes");
    assert_eq!(json["x_custom"], "hello");
    // Served over a real socket, the proxy appends the peer address.
    assert!(json["x_forwarded_for"]
        .as_str()
        .unwrap_or("")
        .contains("127.0.0.1"));
}

#[tokio::test]
async fn proxy_relays_status_and_multi_value_headers() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base).await;
    let client = client();

    let response = client
        .get(format!("{}/api/v1/relay", edge.base))
        .send()
        .await
        .expect("get");
    assert_eq!(response.status(), 201);
    let cookies: Vec<_> = response.headers().get_all("set-cookie").iter().collect();
    assert_eq!(cookies.len(), 2);
    let multi: Vec<_> = response.headers().get_all("x-multi").iter().collect();
    assert_eq!(multi.len(), 2);
    assert_eq!(response.bytes().await.expect("body").as_ref(), b"created");
}

#[tokio::test]
async fn proxy_failure_is_502_error_shape() {
    let edge = spawn_edge("http://127.0.0.1:1").await;
    let client = client();

    let response = client
        .get(format!("{}/api/v1/things", edge.base))
        .send()
        .await
        .expect("get");
    assert_eq!(response.status(), 502);
    let body = response.bytes().await.expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"]["code"], "bad_gateway");
}

#[tokio::test]
async fn rollback_drill() {
    // The operational drill, end to end: serve from Django, flip to Rust,
    // flip back. Each leg asserts which side answered by the response body.
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base).await;
    let client = client();
    let url = format!("{}/", edge.base);

    async fn root_body(client: &reqwest::Client, url: &str) -> Vec<u8> {
        client
            .get(url)
            .send()
            .await
            .expect("get /")
            .bytes()
            .await
            .expect("body")
            .to_vec()
    }

    // 1. Baseline: flag off, Django answers.
    assert_eq!(root_body(&client, &url).await, b"{\"status\": \"STUB\"}");
    // 2. Cutover: flip on, Rust answers with Django-exact bytes.
    edge.handle.set_flag(Prefix::Web, true);
    assert_eq!(root_body(&client, &url).await, HEALTH_BODY);
    // 3. Rollback: flip off, Django answers again.
    edge.handle.set_flag(Prefix::Web, false);
    assert_eq!(root_body(&client, &url).await, b"{\"status\": \"STUB\"}");

    // Rollback state holds for the other edge path too.
    let robots = client
        .get(format!("{}/robots.txt", edge.base))
        .send()
        .await
        .expect("get robots");
    assert_eq!(robots.bytes().await.expect("body").as_ref(), ROBOTS_BODY);
}
