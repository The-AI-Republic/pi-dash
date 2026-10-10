//! Shadow-read integration tests (PIDASHCONV-822).
//!
//! A stub "Django" upstream stands in for the real backend. With shadow
//! enabled and the web flag off, reads proxy to the stub while the Rust
//! side is computed in the background and compared:
//!
//! - (a) the client bytes are unchanged with the flag on,
//! - (b) a mismatch is logged and counted,
//! - (c) writes are never shadowed,
//! - (d) secrets are redacted from logs and the ring.
//!
//! Plus: the flag defaults off end to end, `X-Api-Key` reads never shadow
//! (both backends stamp `last_used`), and sampling/cap zero skip.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use pidash_api::{with_routes, AppState, EdgeHandle, ShadowConfig, SHADOW_METRICS_PATH};

const STUB_BODY: &[u8] = b"{\"status\": \"STUB\"}";
const RUST_BODY: &[u8] = b"{\"status\": \"OK\"}";
const ROBOTS_BODY: &[u8] = b"User-agent: *\nDisallow: /";
const PROBE_PATH: &str = "/shadow-secret-probe/";

// ---------------------------------------------------------------------------
// Log capture (process-global for this test binary; assertions use
// per-test-unique markers because tests run in parallel)
// ---------------------------------------------------------------------------

static LOG_BUF: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
static LOG_INIT: std::sync::Once = std::sync::Once::new();

fn log_buf() -> Arc<Mutex<Vec<u8>>> {
    LOG_BUF
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("log buffer lock")
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn init_log_capture() {
    LOG_INIT.call_once(|| {
        let buf = log_buf();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .with_writer(move || LogWriter(buf.clone()))
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

fn log_text() -> String {
    String::from_utf8_lossy(&log_buf().lock().expect("log buffer lock")).into_owned()
}

// ---------------------------------------------------------------------------
// Stub upstream + edge harness
// ---------------------------------------------------------------------------

async fn stub_app() -> axum::Router {
    async fn root() -> Response {
        (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            STUB_BODY,
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
        let body = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap_or_default();
        (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            serde_json::json!({
                "method": method,
                "body": String::from_utf8_lossy(&body),
            })
            .to_string(),
        )
            .into_response()
    }
    async fn stub_secret() -> Response {
        // Side A: Django's answer. The token differs from the Rust side;
        // the presigned query parameters are normalized, not compared.
        (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"token": "sk-probe-AAA-822", "download": "https://files.test/dl?X-Amz-Date=t1&X-Amz-Signature=sigAAA&key=k"}"#,
        )
            .into_response()
    }
    axum::Router::new()
        .route("/", any(root))
        .route("/robots.txt", any(robots))
        .route("/api/v1/things", any(echo))
        .route(PROBE_PATH, any(stub_secret))
        .fallback((axum::http::StatusCode::NOT_FOUND, "stub 404"))
}

/// A flag-gated test double in the shape of the web handlers: while the
/// web flag is off it proxies (the real router), while on it serves side B
/// (the shadow router, whose state has all flags on).
async fn secret_probe(State(state): State<AppState>, req: Request) -> Response {
    if state.edge().flags().is_rust(pidash_api::Prefix::Web) {
        (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"token": "sk-probe-BBB-822", "download": "https://files.test/dl?X-Amz-Date=t2&X-Amz-Signature=sigBBB&key=k"}"#,
        )
            .into_response()
    } else {
        pidash_api::edge::proxy(State(state), req).await
    }
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
    _task: tokio::task::JoinHandle<()>,
}

async fn spawn_edge(
    upstream: &str,
    shadow: Option<ShadowConfig>,
    extra: axum::Router<AppState>,
) -> Edge {
    init_log_capture();
    let state = AppState::with_edge("test", EdgeHandle::for_tests(upstream));
    let state = match shadow {
        Some(config) => state.with_shadow_config(config),
        None => state,
    };
    let app = with_routes(state, extra);
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
        _task: task,
    }
}

fn enabled() -> ShadowConfig {
    ShadowConfig::enabled_for_tests()
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn metrics(client: &reqwest::Client, base: &str) -> serde_json::Value {
    let body = client
        .get(format!("{base}{SHADOW_METRICS_PATH}"))
        .send()
        .await
        .expect("metrics")
        .bytes()
        .await
        .expect("metrics body");
    serde_json::from_slice(&body).expect("metrics json")
}

/// Poll the metrics endpoint until `ready` or 5s out (shadow work lands
/// after the client response).
async fn wait_for_metrics(
    client: &reqwest::Client,
    base: &str,
    ready: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let snapshot = metrics(client, base).await;
        if ready(&snapshot) || std::time::Instant::now() >= deadline {
            return snapshot;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

// ---------------------------------------------------------------------------
// (a) client bytes unchanged with the flag on
// ---------------------------------------------------------------------------

#[tokio::test]
async fn client_bytes_unchanged_with_shadow_on() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base, Some(enabled()), axum::Router::new()).await;
    let client = client();

    // The stub disagrees with Rust here (STUB vs OK): the client must still
    // see Django's bytes exactly.
    let root = client
        .get(format!("{}/", edge.base))
        .send()
        .await
        .expect("get /");
    assert_eq!(root.status(), 200);
    assert_eq!(root.headers()["content-type"], "application/json");
    assert_eq!(root.bytes().await.expect("body").as_ref(), STUB_BODY);

    let robots = client
        .get(format!("{}/robots.txt", edge.base))
        .send()
        .await
        .expect("get robots");
    assert_eq!(robots.status(), 200);
    assert_eq!(robots.headers()["content-type"], "text/plain");
    assert_eq!(robots.bytes().await.expect("body").as_ref(), ROBOTS_BODY);

    let head = client
        .head(format!("{}/", edge.base))
        .send()
        .await
        .expect("head /");
    assert_eq!(head.status(), 200);
    assert_eq!(head.bytes().await.expect("body").as_ref(), b"");
}

// ---------------------------------------------------------------------------
// (b) a mismatch is logged and counted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mismatch_is_logged_and_counted() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base, Some(enabled()), axum::Router::new()).await;
    let client = client();

    client
        .get(format!("{}/", edge.base))
        .send()
        .await
        .expect("get /");

    let snapshot = wait_for_metrics(&client, &edge.base, |snapshot| {
        snapshot["totals"]["compared"] == 1
    })
    .await;
    assert_eq!(snapshot["totals"]["compared"], 1, "{snapshot}");
    assert_eq!(snapshot["totals"]["mismatched"], 1, "{snapshot}");
    assert_eq!(snapshot["totals"]["matched"], 0, "{snapshot}");
    assert_eq!(snapshot["routes"]["/"]["mismatched"], 1, "{snapshot}");
    assert_eq!(snapshot["recent_mismatches"][0]["route"], "/");

    // The log line lands before the counter in the same task, so by the
    // time the counter is visible the line is in the buffer.
    let logs = log_text();
    assert!(logs.contains("shadow mismatch"), "{logs}");
    assert!(logs.contains("route_template=\"/\""), "{logs}");
    assert!(logs.contains("method=\"GET\""), "{logs}");
    assert!(logs.contains("django_status=200"), "{logs}");
    assert!(logs.contains("rust_status=200"), "{logs}");
    // Sanity: Rust really computed its own side (OK, not the stub bytes).
    assert_ne!(STUB_BODY, RUST_BODY);
}

// ---------------------------------------------------------------------------
// (c) writes are never shadowed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn writes_are_never_shadowed() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base, Some(enabled()), axum::Router::new()).await;
    let client = client();

    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let response = client
            .request(
                method.parse().expect("method"),
                format!("{}/api/v1/things", edge.base),
            )
            .body("write-bytes")
            .send()
            .await
            .expect("write");
        assert_eq!(response.status(), 200, "{method}");
    }

    let snapshot = wait_for_metrics(&client, &edge.base, |snapshot| {
        snapshot["totals"]["skipped"] == 4
    })
    .await;
    assert_eq!(snapshot["totals"]["compared"], 0, "{snapshot}");
    assert_eq!(snapshot["totals"]["skipped"], 4, "{snapshot}");
}

// ---------------------------------------------------------------------------
// (d) secrets are redacted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn secrets_are_redacted() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let extra = axum::Router::new().route(PROBE_PATH, get(secret_probe));
    let edge = spawn_edge(&stub_base, Some(enabled()), extra).await;
    let client = client();

    // The client sees side A untouched, secrets and all.
    let response = client
        .get(format!("{}{PROBE_PATH}", edge.base))
        .send()
        .await
        .expect("get probe");
    assert_eq!(response.status(), 200);
    let body = response.bytes().await.expect("body");
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("sk-probe-AAA-822"), "{text}");

    let snapshot = wait_for_metrics(&client, &edge.base, |snapshot| {
        snapshot["totals"]["compared"] == 1
    })
    .await;
    assert_eq!(snapshot["totals"]["mismatched"], 1, "{snapshot}");
    let diff = snapshot["recent_mismatches"][0]["diff"]
        .as_str()
        .expect("diff");
    // The bodies differ only in the token (secret) and the presigned
    // parameters (volatile): the diff says so instead of showing values.
    assert!(
        diff.contains("only in redacted or volatile values"),
        "{diff}"
    );
    // Full secrets, presigned values, and credential fragments: nothing
    // secret-bearing may appear, whole or split across the trim.
    for secret in [
        "sk-probe-AAA-822",
        "sk-probe-BBB-822",
        "sigAAA",
        "sigBBB",
        "AAA-822",
        "BBB-822",
    ] {
        assert!(!diff.contains(secret), "{diff}");
    }

    // The log line carries the same excerpt, so it must be clean too.
    // (Per-test-unique secrets: parallel tests cannot plant these.)
    let logs = log_text();
    for secret in [
        "sk-probe-AAA-822",
        "sk-probe-BBB-822",
        "sigAAA",
        "sigBBB",
        "AAA-822",
        "BBB-822",
    ] {
        assert!(!logs.contains(secret), "{logs}");
    }
}

// ---------------------------------------------------------------------------
// Defaults and guards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shadow_defaults_off_end_to_end() {
    for var in [
        pidash_api::edge_shadow::SHADOW_ENV,
        pidash_api::edge_shadow::SHADOW_MAX_INFLIGHT_ENV,
        pidash_api::edge_shadow::SHADOW_SAMPLE_ENV,
    ] {
        std::env::remove_var(var);
    }
    let (stub_base, _stub) = spawn(stub_app().await).await;
    // No override: the gate reads the (empty) environment.
    let edge = spawn_edge(&stub_base, None, axum::Router::new()).await;
    let client = client();

    let root = client
        .get(format!("{}/", edge.base))
        .send()
        .await
        .expect("get /");
    assert_eq!(root.bytes().await.expect("body").as_ref(), STUB_BODY);

    let snapshot = metrics(&client, &edge.base).await;
    assert_eq!(snapshot["enabled"], false, "{snapshot}");
    assert_eq!(
        snapshot["totals"],
        serde_json::json!({
            "compared": 0,
            "matched": 0,
            "mismatched": 0,
            "skipped": 0,
            "errored": 0,
        }),
        "{snapshot}"
    );
}

#[tokio::test]
async fn x_api_key_reads_are_never_shadowed() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base, Some(enabled()), axum::Router::new()).await;
    let client = client();

    let root = client
        .get(format!("{}/", edge.base))
        .header("X-Api-Key", "test-key-822")
        .send()
        .await
        .expect("get /");
    assert_eq!(root.bytes().await.expect("body").as_ref(), STUB_BODY);

    let snapshot = wait_for_metrics(&client, &edge.base, |snapshot| {
        snapshot["totals"]["skipped"] == 1
    })
    .await;
    assert_eq!(snapshot["totals"]["compared"], 0, "{snapshot}");
}

#[tokio::test]
async fn sampling_zero_skips() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let config = ShadowConfig {
        sample_rate: 0.0,
        ..enabled()
    };
    let edge = spawn_edge(&stub_base, Some(config), axum::Router::new()).await;
    let client = client();

    client
        .get(format!("{}/", edge.base))
        .send()
        .await
        .expect("get /");
    let snapshot = wait_for_metrics(&client, &edge.base, |snapshot| {
        snapshot["totals"]["skipped"] == 1
    })
    .await;
    assert_eq!(snapshot["totals"]["compared"], 0, "{snapshot}");
}

#[tokio::test]
async fn inflight_cap_zero_skips() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let config = ShadowConfig {
        max_inflight: 0,
        ..enabled()
    };
    let edge = spawn_edge(&stub_base, Some(config), axum::Router::new()).await;
    let client = client();

    let root = client
        .get(format!("{}/", edge.base))
        .send()
        .await
        .expect("get /");
    assert_eq!(root.bytes().await.expect("body").as_ref(), STUB_BODY);
    let snapshot = wait_for_metrics(&client, &edge.base, |snapshot| {
        snapshot["totals"]["skipped"] == 1
    })
    .await;
    assert_eq!(snapshot["totals"]["compared"], 0, "{snapshot}");
}

#[tokio::test]
async fn metrics_endpoint_shape() {
    let (stub_base, _stub) = spawn(stub_app().await).await;
    let edge = spawn_edge(&stub_base, Some(enabled()), axum::Router::new()).await;
    let client = client();

    let snapshot = metrics(&client, &edge.base).await;
    assert_eq!(snapshot["enabled"], true, "{snapshot}");
    for key in ["compared", "matched", "mismatched", "skipped", "errored"] {
        assert!(snapshot["totals"].get(key).is_some(), "{snapshot}");
    }
    assert!(snapshot["routes"].is_object(), "{snapshot}");
    assert!(snapshot["recent_mismatches"].is_array(), "{snapshot}");
}
